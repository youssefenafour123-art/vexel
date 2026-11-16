//! One task per joined Sector: owns the gossip subscription and the [`Roster`].

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use iroh::EndpointId;
use iroh_gossip::api::{Event, GossipReceiver, GossipSender};
use n0_future::StreamExt;
use tokio::sync::{mpsc, oneshot};
use vxl_core::roster::PRESENCE_INTERVAL_MS;
use vxl_core::wire::{self, GossipKind, GossipMsg, Presence};
use vxl_core::{InviteCap, MemberView, PeerAddr, PeerId, Roster, RosterChange, Sector};

use crate::{NetEvent, Shared, convert, now_ms};

const EXPIRE_TICK: Duration = Duration::from_secs(1);
const REJOIN_TICK: Duration = Duration::from_secs(10);
const LEAVE_FLUSH: Duration = Duration::from_millis(250);

pub(crate) enum SectorCmd {
    NameChanged,
    Members(oneshot::Sender<Vec<MemberView>>),
    Leave(oneshot::Sender<()>),
}

pub(crate) struct SectorTask {
    pub(crate) shared: Arc<Shared>,
    pub(crate) sector: Sector,
    pub(crate) cap: Option<InviteCap>,
    pub(crate) sender: GossipSender,
    pub(crate) receiver: GossipReceiver,
    pub(crate) candidates: HashSet<EndpointId>,
    pub(crate) known: Vec<(PeerId, String)>,
    pub(crate) cmds: mpsc::Receiver<SectorCmd>,
}

impl SectorTask {
    pub(crate) async fn run(mut self) {
        let me = self.shared.identity.peer_id();
        let mut roster = Roster::new(self.sector.clone(), me, &self.shared.display_name());
        for (peer, name) in &self.known {
            roster.seed_member(*peer, name);
        }
        let mut last_members = Vec::new();
        self.emit_roster(&roster, &mut last_members);

        let mut presence_iv = tokio::time::interval(Duration::from_millis(PRESENCE_INTERVAL_MS));
        let mut expire_iv = tokio::time::interval(EXPIRE_TICK);
        let mut rejoin_iv = tokio::time::interval(REJOIN_TICK);
        rejoin_iv.tick().await; // The first tick is immediate; skip it, since we just joined.

        loop {
            tokio::select! {
                _ = presence_iv.tick() => self.send_presence().await,
                _ = expire_iv.tick() => {
                    for peer in roster.expire(now_ms()) {
                        self.shared.links.untrack(self.sector.id, &peer);
                    }
                    self.emit_roster(&roster, &mut last_members);
                }
                _ = rejoin_iv.tick() => {
                    if self.receiver.neighbors().next().is_none() && !self.candidates.is_empty() {
                        let peers: Vec<EndpointId> = self.candidates.iter().copied().collect();
                        if let Err(err) = self.sender.join_peers(peers).await {
                            tracing::debug!(%err, "gossip rejoin failed");
                        }
                    }
                }
                ev = self.receiver.next() => match ev {
                    None => {
                        tracing::warn!(sector = %self.sector.id.short(), "gossip topic closed");
                        break;
                    }
                    Some(Err(err)) => {
                        tracing::warn!(sector = %self.sector.id.short(), %err, "gossip receive error");
                        break;
                    }
                    Some(Ok(Event::NeighborUp(_) | Event::Lagged)) => self.send_presence().await,
                    Some(Ok(Event::Received(msg))) => {
                        self.on_message(&mut roster, &msg.content);
                        self.emit_roster(&roster, &mut last_members);
                    }
                    Some(Ok(_)) => {}
                },
                cmd = self.cmds.recv() => match cmd {
                    Some(SectorCmd::NameChanged) => {
                        roster.set_my_name(&self.shared.display_name());
                        self.send_presence().await;
                        self.emit_roster(&roster, &mut last_members);
                    }
                    Some(SectorCmd::Members(tx)) => {
                        let _ = tx.send(roster.members());
                    }
                    Some(SectorCmd::Leave(done)) => {
                        self.broadcast(GossipKind::Leave).await;
                        // `broadcast` only enqueues; give the gossip actor a moment to flush the
                        // Leave before our handles drop and the topic closes.
                        tokio::time::sleep(LEAVE_FLUSH).await;
                        self.shared.links.untrack_sector(self.sector.id);
                        let _ = done.send(());
                        return;
                    }
                    None => break,
                },
            }
        }
        self.shared.links.untrack_sector(self.sector.id);
    }

    fn on_message(&mut self, roster: &mut Roster, content: &[u8]) {
        let (author, msg) = match wire::open(content) {
            Ok(v) => v,
            Err(err) => {
                tracing::debug!(%err, "dropping unverifiable gossip");
                return;
            }
        };
        let now = now_ms();
        match roster.apply(author, &msg, now) {
            Ok(RosterChange::Online(peer) | RosterChange::Refreshed(peer)) => {
                if let GossipKind::Presence(p) = &msg.kind {
                    self.learn(&p.addr);
                    let display_name = roster.name_of(&peer).unwrap_or_default().to_owned();
                    self.shared.emit(NetEvent::MemberSeen {
                        sector_id: self.sector.id,
                        addr: p.addr.clone(),
                        display_name,
                        seen_ms: now,
                    });
                }
            }
            Ok(RosterChange::Offline(peer)) => self.shared.links.untrack(self.sector.id, &peer),
            Ok(RosterChange::Unchanged) => {}
            Err(err) => {
                tracing::debug!(peer = %author.short(), %err, "rejected gossip from peer");
            }
        }
    }

    /// Makes a member's address dialable and starts measuring the link to it.
    fn learn(&mut self, addr: &PeerAddr) {
        if let Some(ea) = convert::to_endpoint_addr(addr) {
            self.candidates.insert(ea.id);
            self.shared.lookup.add_endpoint_info(ea);
        }
        self.shared.links.track(self.sector.id, addr.clone());
    }

    async fn send_presence(&mut self) {
        let Some(addr) = convert::from_endpoint_addr(&self.shared.endpoint.addr()) else {
            return;
        };
        let presence = Presence {
            display_name: self.shared.display_name(),
            addr,
            cap: self.cap.clone(),
        };
        self.broadcast(GossipKind::Presence(Box::new(presence)))
            .await;
    }

    async fn broadcast(&mut self, kind: GossipKind) {
        let msg = GossipMsg {
            sector_id: self.sector.id,
            ts_ms: self.shared.next_ts(),
            kind,
        };
        match wire::seal(&self.shared.identity, &msg) {
            Ok(bytes) => {
                if let Err(err) = self.sender.broadcast(bytes.into()).await {
                    tracing::debug!(%err, "gossip broadcast failed");
                }
            }
            Err(err) => tracing::warn!(%err, "could not seal gossip message"),
        }
    }

    fn emit_roster(&self, roster: &Roster, last: &mut Vec<MemberView>) {
        let members = roster.members();
        if members != *last {
            last.clone_from(&members);
            self.shared.emit(NetEvent::Roster {
                sector_id: self.sector.id,
                members,
            });
        }
    }
}
