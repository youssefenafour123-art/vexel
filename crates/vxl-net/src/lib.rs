//! # vxl-net
//!
//! The VEXEL peer: an iroh [`Endpoint`] (QUIC, hole punching, relay fallback) plus:
//! - one iroh-gossip topic per Sector (topic id = `SectorId`), carrying signed presence;
//! - the Phase 1 ACL (owner + invite cap) via [`vxl_core::Roster`];
//! - per-peer RTT and loss over QUIC datagrams ([`link`]);
//! - real socket byte counters fed into [`CoreMetrics`].
//!
//! The crate never touches storage or UI. It reports what happens through [`NetEvent`]s, and the
//! caller (desktop shell or `vxl-cli`) persists and displays them.
//!
//! Every async method must be called from inside a Tokio runtime.

mod convert;
pub mod link;
mod sector;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, RelayMode, RelayUrl, SecretKey};
use iroh_gossip::Gossip;
use tokio::sync::{mpsc, oneshot};
use vxl_core::sector::clean_display_name;
use vxl_core::{Identity, InviteCap, MemberView, PeerAddr, PeerId, Sector, SectorId, SectorKey};
use vxl_metrics::CoreMetrics;

pub use link::{LinkStats, PathKind};

use crate::link::{Links, PING_ALPN, PingProtocol};
use crate::sector::{SectorCmd, SectorTask};

/// Default lifetime of a freshly issued owner invite.
pub const DEFAULT_INVITE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
/// How long [`Node::wait_online`] callers should usually wait for a home relay.
pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);

const EVENT_CAPACITY: usize = 1024;
const LINK_TICK: Duration = Duration::from_secs(1);
const BYTES_TICK: Duration = Duration::from_millis(250);
const LEAVE_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_KEY_PEERS: usize = 3;

/// Errors from `vxl-net`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not bind the network endpoint: {0}")]
    Bind(String),
    #[error("invalid relay URL")]
    BadRelayUrl,
    #[error("gossip error: {0}")]
    Gossip(String),
    #[error(transparent)]
    Core(#[from] vxl_core::Error),
    #[error("OS random number generator failed")]
    Rng,
    #[error("already joined this Sector")]
    AlreadyJoined,
    #[error("not joined to this Sector")]
    NotJoined,
}

/// Which relay servers to use.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RelayChoice {
    /// n0's public relays (best effort, no SLA).
    #[default]
    Default,
    /// A self-hosted `iroh-relay`.
    Custom(String),
    /// Direct connections only (LAN or hole punching; no fallback).
    Disabled,
}

/// Node configuration.
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub identity: Identity,
    pub display_name: String,
    pub relay: RelayChoice,
    pub metrics: Arc<CoreMetrics>,
}

/// Everything the node reports to its owner.
#[derive(Debug, Clone)]
pub enum NetEvent {
    /// The endpoint has a home relay (or gave up waiting) and knows its addresses.
    Online { addr: PeerAddr },
    /// Membership snapshot of a Sector; sent whenever it changes.
    Roster {
        sector_id: SectorId,
        members: Vec<MemberView>,
    },
    /// An admitted member announced itself. Persist the member and its address.
    MemberSeen {
        sector_id: SectorId,
        addr: PeerAddr,
        display_name: String,
        seen_ms: u64,
    },
    /// Link quality to every tracked peer (1 Hz).
    Links(Vec<LinkStats>),
}

/// State shared by the node, its Sector tasks and its link manager.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) identity: Identity,
    pub(crate) endpoint: Endpoint,
    pub(crate) lookup: MemoryLookup,
    pub(crate) links: Arc<Links>,
    display_name: RwLock<String>,
    events: mpsc::Sender<NetEvent>,
    last_ts: AtomicU64,
}

impl Shared {
    pub(crate) fn display_name(&self) -> String {
        self.display_name
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Never blocks: if the consumer falls 1024 events behind, events are dropped (snapshots self-heal).
    pub(crate) fn emit(&self, ev: NetEvent) {
        if self.events.try_send(ev).is_err() {
            tracing::debug!("net event channel full; dropping event");
        }
    }

    /// Strictly increasing wall-clock timestamp for our gossip messages.
    pub(crate) fn next_ts(&self) -> u64 {
        let now = now_ms();
        let mut cur = self.last_ts.load(Ordering::Relaxed);
        loop {
            let next = now.max(cur + 1);
            match self.last_ts.compare_exchange_weak(
                cur,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return next,
                Err(actual) => cur = actual,
            }
        }
    }
}

#[derive(Debug)]
struct Joined {
    cmds: mpsc::Sender<SectorCmd>,
}

/// A running VEXEL peer. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Node {
    shared: Arc<Shared>,
    router: Router,
    gossip: Gossip,
    sectors: Arc<Mutex<HashMap<SectorId, Joined>>>,
    background: Arc<Vec<tokio::task::AbortHandle>>,
}

impl Node {
    /// Binds the endpoint and starts the protocols. Returns the node and its event stream.
    pub async fn start(cfg: NetConfig) -> Result<(Self, mpsc::Receiver<NetEvent>), Error> {
        let relay_mode = match &cfg.relay {
            RelayChoice::Default => RelayMode::Default,
            RelayChoice::Disabled => RelayMode::Disabled,
            RelayChoice::Custom(url) => RelayMode::Custom(
                url.parse::<RelayUrl>()
                    .map_err(|_| Error::BadRelayUrl)?
                    .into(),
            ),
        };
        let lookup = MemoryLookup::new();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&cfg.identity.seed()))
            .address_lookup(lookup.clone())
            .relay_mode(relay_mode)
            .bind()
            .await
            .map_err(|e| Error::Bind(e.to_string()))?;

        let gossip = Gossip::builder().spawn(endpoint.clone());
        let links = Links::new(
            cfg.identity.peer_id(),
            endpoint.clone(),
            Arc::clone(&cfg.metrics),
        );
        let router = Router::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(PING_ALPN, PingProtocol(Arc::clone(&links)))
            .spawn();

        let (tx, rx) = mpsc::channel(EVENT_CAPACITY);
        let shared = Arc::new(Shared {
            identity: cfg.identity,
            endpoint: endpoint.clone(),
            lookup,
            links,
            display_name: RwLock::new(cfg.display_name),
            events: tx,
            last_ts: AtomicU64::new(0),
        });

        let background = vec![
            tokio::spawn(online_task(Arc::clone(&shared))).abort_handle(),
            tokio::spawn(link_task(Arc::clone(&shared))).abort_handle(),
            tokio::spawn(bytes_task(endpoint, cfg.metrics)).abort_handle(),
        ];

        Ok((
            Self {
                shared,
                router,
                gossip,
                sectors: Arc::default(),
                background: Arc::new(background),
            },
            rx,
        ))
    }

    pub fn peer_id(&self) -> PeerId {
        self.shared.identity.peer_id()
    }

    /// Our current dialing address (relay URL + direct addresses).
    pub fn addr(&self) -> Option<PeerAddr> {
        convert::from_endpoint_addr(&self.shared.endpoint.addr())
    }

    /// Waits until the endpoint has a home relay, up to `timeout`. Returns whether it got one.
    pub async fn wait_online(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, self.shared.endpoint.online())
            .await
            .is_ok()
    }

    /// Changes our display name and re-announces it in every Sector.
    pub fn set_display_name(&self, name: &str) -> Result<String, Error> {
        let name = clean_display_name(name)?;
        *self
            .shared
            .display_name
            .write()
            .unwrap_or_else(PoisonError::into_inner) = name.clone();
        for j in self.lock_sectors().values() {
            let _ = j.cmds.try_send(SectorCmd::NameChanged);
        }
        Ok(name)
    }

    /// Joins a Sector's gossip swarm.
    ///
    /// - `cap`: the invite we joined with (`None` if we own the Sector).
    /// - `bootstrap`: peers to dial first (from the Sector Key and/or stored addresses).
    /// - `known`: previously admitted members, so they show up (offline) right away.
    pub async fn join(
        &self,
        sector: Sector,
        cap: Option<InviteCap>,
        bootstrap: Vec<PeerAddr>,
        known: Vec<(PeerId, String)>,
    ) -> Result<(), Error> {
        if self.lock_sectors().contains_key(&sector.id) {
            return Err(Error::AlreadyJoined);
        }
        let me = self.peer_id();
        let mut candidates = HashSet::new();
        for addr in bootstrap.iter().filter(|a| a.id != me) {
            if let Some(ea) = convert::to_endpoint_addr(addr) {
                candidates.insert(ea.id);
                self.shared.lookup.add_endpoint_info(ea);
            }
        }
        let topic = self
            .gossip
            .subscribe(
                convert::topic(&sector.id),
                candidates.iter().copied().collect(),
            )
            .await
            .map_err(|e| Error::Gossip(e.to_string()))?;
        let (sender, receiver) = topic.split();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        {
            let mut sectors = self.lock_sectors();
            if sectors.contains_key(&sector.id) {
                return Err(Error::AlreadyJoined);
            }
            sectors.insert(sector.id, Joined { cmds: cmd_tx });
        }

        let id = sector.id;
        let task = SectorTask {
            shared: Arc::clone(&self.shared),
            sector,
            cap,
            sender,
            receiver,
            candidates,
            known,
            cmds: cmd_rx,
        };
        let sectors = Arc::clone(&self.sectors);
        tokio::spawn(async move {
            task.run().await;
            sectors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
        });
        Ok(())
    }

    /// Leaves a Sector, announcing it to peers first.
    pub async fn leave(&self, id: &SectorId) -> Result<(), Error> {
        let cmds = self
            .lock_sectors()
            .remove(id)
            .map(|j| j.cmds)
            .ok_or(Error::NotJoined)?;
        let (tx, rx) = oneshot::channel();
        if cmds.send(SectorCmd::Leave(tx)).await.is_ok() {
            let _ = tokio::time::timeout(LEAVE_TIMEOUT, rx).await;
        }
        Ok(())
    }

    /// The current member list of a joined Sector.
    pub async fn members(&self, id: &SectorId) -> Result<Vec<MemberView>, Error> {
        let cmds = self
            .lock_sectors()
            .get(id)
            .map(|j| j.cmds.clone())
            .ok_or(Error::NotJoined)?;
        let (tx, rx) = oneshot::channel();
        cmds.send(SectorCmd::Members(tx))
            .await
            .map_err(|_| Error::NotJoined)?;
        rx.await.map_err(|_| Error::NotJoined)
    }

    /// Builds a shareable Sector Key.
    ///
    /// The owner signs a fresh invite (expiring after `ttl`, or never with `None`). A member re-shares
    /// the owner-signed invite it joined with. Bootstrap hints are our own address plus a few peers we
    /// have a live link to.
    pub fn sector_key(
        &self,
        sector: &Sector,
        joined_with: Option<&InviteCap>,
        ttl: Option<Duration>,
    ) -> Result<SectorKey, Error> {
        let cap = if sector.owner == self.peer_id() {
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).map_err(|_| Error::Rng)?;
            let expires = ttl.map(|t| now_ms().saturating_add(t.as_millis() as u64));
            InviteCap::issue(&self.shared.identity, sector, expires, nonce)?
        } else {
            joined_with.cloned().ok_or(vxl_core::Error::NotOwner)?
        };
        let mut boot: Vec<PeerAddr> = self.addr().into_iter().collect();
        boot.extend(self.shared.links.connected_in(&sector.id, MAX_KEY_PEERS));
        Ok(SectorKey::new(cap, boot))
    }

    /// IDs of joined Sectors.
    pub fn joined(&self) -> Vec<SectorId> {
        self.lock_sectors().keys().copied().collect()
    }

    /// Leaves every Sector (announcing it) and closes the endpoint.
    pub async fn shutdown(&self) {
        let ids = self.joined();
        for id in ids {
            let _ = self.leave(&id).await;
        }
        for h in self.background.iter() {
            h.abort();
        }
        if let Err(err) = self.router.shutdown().await {
            tracing::debug!(%err, "router shutdown error");
        }
    }

    fn lock_sectors(&self) -> std::sync::MutexGuard<'_, HashMap<SectorId, Joined>> {
        self.sectors.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Creates a new Sector owned by `owner`, with a random id.
pub fn new_sector(owner: &Identity, name: &str) -> Result<Sector, Error> {
    let mut id = [0u8; 32];
    getrandom::fill(&mut id).map_err(|_| Error::Rng)?;
    Ok(Sector::new(id, name, owner.peer_id())?)
}

/// Wall clock in Unix milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

async fn online_task(shared: Arc<Shared>) {
    let _ = tokio::time::timeout(ONLINE_TIMEOUT, shared.endpoint.online()).await;
    if let Some(addr) = convert::from_endpoint_addr(&shared.endpoint.addr()) {
        shared.emit(NetEvent::Online { addr });
    }
}

async fn link_task(shared: Arc<Shared>) {
    let mut iv = tokio::time::interval(LINK_TICK);
    loop {
        iv.tick().await;
        let stats = shared.links.tick();
        shared.emit(NetEvent::Links(stats));
    }
}

/// Mirrors the endpoint's socket byte totals (all transports) into the core counters.
async fn bytes_task(endpoint: Endpoint, metrics: Arc<CoreMetrics>) {
    let mut iv = tokio::time::interval(BYTES_TICK);
    let (mut last_tx, mut last_rx) = (0u64, 0u64);
    loop {
        iv.tick().await;
        let s = &endpoint.metrics().socket;
        let tx = s.send_ipv4.get() + s.send_ipv6.get() + s.send_relay.get();
        let rx = s.recv_data_ipv4.get()
            + s.recv_data_ipv6.get()
            + s.recv_data_relay.get()
            + s.recv_data_custom.get();
        metrics.tx_bytes.add(tx.saturating_sub(last_tx));
        metrics.rx_bytes.add(rx.saturating_sub(last_rx));
        last_tx = tx;
        last_rx = rx;
    }
}
