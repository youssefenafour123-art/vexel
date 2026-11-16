//! Presence-driven membership for one Sector, with the Phase 1 ACL.
//!
//! **Admission rule (Phase 1):** a peer is a member if it is the owner, if it was admitted before
//! (persisted members are seeded back in), or if its presence carries a valid owner-signed
//! [`InviteCap`](crate::InviteCap) for this Sector that has not expired. Expiry only limits *new*
//! members; once admitted, a member stays admitted. Revocation and role ops arrive with the signed
//! ACL log.
//!
//! Liveness: a member is online while presence keeps arriving. After [`PRESENCE_TTL_MS`] of silence
//! [`Roster::expire`] marks it offline.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::sector::clean_display_name;
use crate::wire::{GossipKind, GossipMsg};
use crate::{Error, PeerAddr, PeerId, Role, Sector};

/// How often a peer re-announces presence.
pub const PRESENCE_INTERVAL_MS: u64 = 4_000;
/// Silence after which a member is shown offline (three missed announcements).
pub const PRESENCE_TTL_MS: u64 = 3 * PRESENCE_INTERVAL_MS;

#[derive(Debug, Clone)]
struct Entry {
    display_name: String,
    role: Role,
    online: bool,
    last_seen_ms: u64,
    last_ts_ms: u64,
    addr: Option<PeerAddr>,
}

/// What a message changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterChange {
    /// A member came online (new, or back from offline).
    Online(PeerId),
    /// An online member refreshed its presence (name or address may have changed).
    Refreshed(PeerId),
    /// A member left gracefully.
    Offline(PeerId),
    /// Stale, duplicate or no-op.
    Unchanged,
}

/// A member as shown to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    #[serde(serialize_with = "ser_display")]
    pub peer: PeerId,
    pub short_id: String,
    pub display_name: String,
    pub role: Role,
    pub online: bool,
    pub is_self: bool,
}

fn ser_display<S: serde::Serializer>(p: &PeerId, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(p)
}

/// Membership and liveness for one Sector.
#[derive(Debug, Clone)]
pub struct Roster {
    sector: Sector,
    me: PeerId,
    members: BTreeMap<PeerId, Entry>,
}

impl Roster {
    /// Creates a roster containing only ourselves (always online).
    pub fn new(sector: Sector, me: PeerId, my_name: &str) -> Self {
        let mut members = BTreeMap::new();
        members.insert(
            me,
            Entry {
                display_name: my_name.to_owned(),
                role: sector.role_of(&me),
                online: true,
                last_seen_ms: u64::MAX,
                last_ts_ms: 0,
                addr: None,
            },
        );
        Self {
            sector,
            me,
            members,
        }
    }

    pub fn sector(&self) -> &Sector {
        &self.sector
    }

    /// Re-adds a previously admitted member (from storage) as offline.
    pub fn seed_member(&mut self, peer: PeerId, display_name: &str) {
        if peer == self.me {
            return;
        }
        let role = self.sector.role_of(&peer);
        self.members.entry(peer).or_insert_with(|| Entry {
            display_name: display_name.to_owned(),
            role,
            online: false,
            last_seen_ms: 0,
            last_ts_ms: 0,
            addr: None,
        });
    }

    /// Updates our own display name.
    pub fn set_my_name(&mut self, name: &str) {
        if let Some(e) = self.members.get_mut(&self.me) {
            e.display_name = name.to_owned();
        }
    }

    /// Applies a verified gossip message from `author`. Errors mean "drop it" (wrong Sector, unauthorized, bad data).
    pub fn apply(
        &mut self,
        author: PeerId,
        msg: &GossipMsg,
        now_ms: u64,
    ) -> Result<RosterChange, Error> {
        if msg.sector_id != self.sector.id {
            return Err(Error::WrongSector);
        }
        if author == self.me {
            return Ok(RosterChange::Unchanged);
        }
        match &msg.kind {
            GossipKind::Presence(p) => {
                if p.addr.id != author {
                    return Err(Error::AddrMismatch);
                }
                let name = clean_display_name(&p.display_name)?;
                let known = self.members.contains_key(&author);
                if !known && author != self.sector.owner {
                    let cap = p.cap.as_ref().ok_or(Error::Unauthorized)?;
                    if cap.sector_id != self.sector.id || cap.owner != self.sector.owner {
                        return Err(Error::Unauthorized);
                    }
                    cap.check(now_ms)?;
                }
                let role = self.sector.role_of(&author);
                let entry = self.members.entry(author).or_insert_with(|| Entry {
                    display_name: name.clone(),
                    role,
                    online: false,
                    last_seen_ms: 0,
                    last_ts_ms: 0,
                    addr: None,
                });
                if known && msg.ts_ms <= entry.last_ts_ms {
                    return Ok(RosterChange::Unchanged);
                }
                let was_online = entry.online;
                entry.display_name = name;
                entry.online = true;
                entry.last_seen_ms = now_ms;
                entry.last_ts_ms = msg.ts_ms;
                entry.addr = Some(p.addr.clone());
                Ok(if was_online {
                    RosterChange::Refreshed(author)
                } else {
                    RosterChange::Online(author)
                })
            }
            GossipKind::Leave => match self.members.get_mut(&author) {
                Some(e) if e.online && msg.ts_ms > e.last_ts_ms => {
                    e.online = false;
                    e.last_ts_ms = msg.ts_ms;
                    Ok(RosterChange::Offline(author))
                }
                _ => Ok(RosterChange::Unchanged),
            },
        }
    }

    /// Marks members offline after [`PRESENCE_TTL_MS`] of silence. Returns who went offline.
    pub fn expire(&mut self, now_ms: u64) -> Vec<PeerId> {
        let mut gone = Vec::new();
        for (peer, e) in &mut self.members {
            if *peer != self.me
                && e.online
                && now_ms.saturating_sub(e.last_seen_ms) > PRESENCE_TTL_MS
            {
                e.online = false;
                gone.push(*peer);
            }
        }
        gone
    }

    /// Online peers other than ourselves, with their last known address.
    pub fn online_peers(&self) -> Vec<PeerAddr> {
        self.members
            .iter()
            .filter(|(p, e)| **p != self.me && e.online)
            .filter_map(|(_, e)| e.addr.clone())
            .collect()
    }

    /// Display name of a member, if known.
    pub fn name_of(&self, peer: &PeerId) -> Option<&str> {
        self.members.get(peer).map(|e| e.display_name.as_str())
    }

    /// All members: owner first, then online, then by name.
    pub fn members(&self) -> Vec<MemberView> {
        let mut out: Vec<MemberView> = self
            .members
            .iter()
            .map(|(p, e)| MemberView {
                peer: *p,
                short_id: p.short(),
                display_name: e.display_name.clone(),
                role: e.role,
                online: e.online,
                is_self: *p == self.me,
            })
            .collect();
        out.sort_by(|a, b| {
            (b.role == Role::Owner)
                .cmp(&(a.role == Role::Owner))
                .then(b.online.cmp(&a.online))
                .then_with(|| {
                    a.display_name
                        .to_lowercase()
                        .cmp(&b.display_name.to_lowercase())
                })
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Presence;
    use crate::{Identity, InviteCap, SectorId};

    struct World {
        owner: Identity,
        joiner: Identity,
        sector: Sector,
        cap: InviteCap,
    }

    fn world() -> World {
        let owner = Identity::from_seed(&[1; 32]);
        let joiner = Identity::from_seed(&[2; 32]);
        let sector = Sector::new([9; 32], "Ops", owner.peer_id()).expect("name");
        let cap = InviteCap::issue(&owner, &sector, Some(1_000), [0; 16]).expect("owner");
        World {
            owner,
            joiner,
            sector,
            cap,
        }
    }

    fn presence(id: &Identity, sector: SectorId, ts: u64, cap: Option<InviteCap>) -> GossipMsg {
        GossipMsg {
            sector_id: sector,
            ts_ms: ts,
            kind: GossipKind::Presence(Box::new(Presence {
                display_name: format!("p{}", id.peer_id().short()),
                addr: PeerAddr {
                    id: id.peer_id(),
                    relay: None,
                    ips: vec![],
                },
                cap,
            })),
        }
    }

    #[test]
    fn owner_admits_invited_joiner() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "owner");
        let m = presence(&w.joiner, w.sector.id, 1, Some(w.cap.clone()));
        assert_eq!(
            r.apply(w.joiner.peer_id(), &m, 500),
            Ok(RosterChange::Online(w.joiner.peer_id()))
        );
        assert_eq!(r.online_peers().len(), 1);
    }

    #[test]
    fn joiner_admits_owner_without_cap() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.joiner.peer_id(), "j");
        let m = presence(&w.owner, w.sector.id, 1, None);
        assert_eq!(
            r.apply(w.owner.peer_id(), &m, 0),
            Ok(RosterChange::Online(w.owner.peer_id()))
        );
        assert_eq!(r.members()[0].role, Role::Owner);
    }

    #[test]
    fn stranger_without_cap_is_rejected() {
        let w = world();
        let stranger = Identity::from_seed(&[7; 32]);
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let m = presence(&stranger, w.sector.id, 1, None);
        assert_eq!(r.apply(stranger.peer_id(), &m, 0), Err(Error::Unauthorized));
    }

    #[test]
    fn expired_cap_rejects_new_member_but_not_known_one() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let m = presence(&w.joiner, w.sector.id, 1, Some(w.cap.clone()));
        assert_eq!(r.apply(w.joiner.peer_id(), &m, 2_000), Err(Error::Expired));
        r.seed_member(w.joiner.peer_id(), "j");
        assert!(matches!(
            r.apply(w.joiner.peer_id(), &m, 2_000),
            Ok(RosterChange::Online(_))
        ));
    }

    #[test]
    fn cap_for_other_sector_is_rejected() {
        let w = world();
        let other = Sector::new([8; 32], "Other", w.owner.peer_id()).expect("name");
        let other_cap = InviteCap::issue(&w.owner, &other, None, [1; 16]).expect("owner");
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let m = presence(&w.joiner, w.sector.id, 1, Some(other_cap));
        assert_eq!(r.apply(w.joiner.peer_id(), &m, 0), Err(Error::Unauthorized));
    }

    #[test]
    fn spoofed_address_is_rejected() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let m = presence(&w.joiner, w.sector.id, 1, Some(w.cap.clone()));
        let impostor = Identity::from_seed(&[5; 32]).peer_id();
        assert_eq!(r.apply(impostor, &m, 0), Err(Error::AddrMismatch));
    }

    #[test]
    fn stale_messages_are_ignored_and_ttl_expires() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let j = w.joiner.peer_id();
        let m5 = presence(&w.joiner, w.sector.id, 5, Some(w.cap.clone()));
        let m4 = presence(&w.joiner, w.sector.id, 4, Some(w.cap.clone()));
        assert!(matches!(r.apply(j, &m5, 100), Ok(RosterChange::Online(_))));
        assert_eq!(r.apply(j, &m4, 200), Ok(RosterChange::Unchanged));
        assert!(r.expire(100 + PRESENCE_TTL_MS).is_empty());
        assert_eq!(r.expire(101 + PRESENCE_TTL_MS), vec![j]);
        assert!(r.online_peers().is_empty());
    }

    #[test]
    fn leave_goes_offline() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let j = w.joiner.peer_id();
        let m = presence(&w.joiner, w.sector.id, 1, Some(w.cap.clone()));
        r.apply(j, &m, 0).expect("admit");
        let leave = GossipMsg {
            sector_id: w.sector.id,
            ts_ms: 2,
            kind: GossipKind::Leave,
        };
        assert_eq!(r.apply(j, &leave, 1), Ok(RosterChange::Offline(j)));
    }

    #[test]
    fn wrong_sector_is_rejected() {
        let w = world();
        let mut r = Roster::new(w.sector.clone(), w.owner.peer_id(), "o");
        let m = presence(
            &w.joiner,
            SectorId::from_bytes([0; 32]),
            1,
            Some(w.cap.clone()),
        );
        assert_eq!(r.apply(w.joiner.peer_id(), &m, 0), Err(Error::WrongSector));
    }
}
