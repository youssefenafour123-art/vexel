//! Per-peer link quality: a tiny ping/echo protocol over **QUIC datagrams** (`vxl/ping/1`).
//!
//! Voice uses datagrams too (Phase 3), so loss measured here is the loss voice will actually see.
//! One connection per peer pair: the peer with the lower id dials, and the other side accepts only
//! peers it already tracks (members it has seen presence from in a shared Sector). Both sides run
//! the same loop on that connection: send `P<seq>` every second, echo any `P` back as `Q`, and time
//! the `Q`s.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bytes::Bytes;
use iroh::Endpoint;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use serde::Serialize;
use vxl_core::{PeerAddr, PeerId, SectorId};
use vxl_metrics::CoreMetrics;

use crate::convert;

/// ALPN of the ping protocol.
pub const PING_ALPN: &[u8] = b"vxl/ping/1";

const PING_INTERVAL: Duration = Duration::from_secs(1);
/// A probe with no echo after this long counts as lost.
const LOSS_TIMEOUT: Duration = Duration::from_secs(2);
/// Probes kept for the loss window (~30 s at 1 Hz).
const WINDOW: usize = 30;
const DIAL_BACKOFF: Duration = Duration::from_secs(3);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const TAG_PING: u8 = b'P';
const TAG_PONG: u8 = b'Q';
const PACKET_LEN: usize = 5;

/// Close code sent to peers we don't know (yet). They retry after [`DIAL_BACKOFF`].
const CLOSE_UNKNOWN: u32 = 1;
const CLOSE_GONE: u32 = 2;

#[derive(Debug, Clone, Copy)]
struct Probe {
    seq: u32,
    sent: Instant,
    acked: bool,
}

/// RTT (EWMA, RFC 6298 style alpha = 1/8) and loss over a sliding window of probes.
#[derive(Debug, Default)]
pub struct PingStats {
    srtt_us: Option<f64>,
    window: VecDeque<Probe>,
}

impl PingStats {
    pub fn on_sent(&mut self, seq: u32, now: Instant) {
        if self.window.len() == WINDOW {
            self.window.pop_front();
        }
        self.window.push_back(Probe {
            seq,
            sent: now,
            acked: false,
        });
    }

    /// Records an echo. Returns the sample RTT, or `None` for unknown or duplicate echoes.
    pub fn on_pong(&mut self, seq: u32, now: Instant) -> Option<Duration> {
        let probe = self.window.iter_mut().find(|p| p.seq == seq && !p.acked)?;
        probe.acked = true;
        let rtt = now.saturating_duration_since(probe.sent);
        let sample = rtt.as_secs_f64() * 1e6;
        self.srtt_us = Some(match self.srtt_us {
            None => sample,
            Some(s) => s + (sample - s) / 8.0,
        });
        Some(rtt)
    }

    /// Smoothed RTT in microseconds.
    pub fn rtt_us(&self) -> Option<u64> {
        self.srtt_us.map(|s| s.round() as u64)
    }

    /// Loss over probes old enough to have been echoed, as a percentage. `None` until one is due.
    pub fn loss_pct(&self, now: Instant) -> Option<f32> {
        let mut due = 0u32;
        let mut lost = 0u32;
        for p in &self.window {
            if now.saturating_duration_since(p.sent) >= LOSS_TIMEOUT {
                due += 1;
                if !p.acked {
                    lost += 1;
                }
            }
        }
        (due > 0).then(|| lost as f32 * 100.0 / due as f32)
    }
}

/// How traffic to a peer currently flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PathKind {
    /// Hole-punched or LAN UDP.
    Direct,
    /// Through a relay server (higher latency).
    Relay,
}

/// Link quality to one peer, sent to the UI at 1 Hz.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStats {
    #[serde(serialize_with = "ser_display")]
    pub peer: PeerId,
    pub connected: bool,
    pub rtt_ms: Option<f32>,
    pub loss_pct: Option<f32>,
    pub path: Option<PathKind>,
}

fn ser_display<S: serde::Serializer>(p: &PeerId, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(p)
}

#[derive(Debug)]
struct Peer {
    sectors: HashSet<SectorId>,
    addr: PeerAddr,
    conn: Option<Connection>,
    stats: Arc<Mutex<PingStats>>,
    dialing: bool,
    last_dial: Option<Instant>,
}

/// Registry of tracked peers and their ping connections.
#[derive(Debug)]
pub(crate) struct Links {
    me: PeerId,
    endpoint: Endpoint,
    peers: Mutex<HashMap<PeerId, Peer>>,
    metrics: Arc<CoreMetrics>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding these locks can't leave the data half-updated in a harmful way.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Links {
    pub(crate) fn new(me: PeerId, endpoint: Endpoint, metrics: Arc<CoreMetrics>) -> Arc<Self> {
        Arc::new(Self {
            me,
            endpoint,
            peers: Mutex::new(HashMap::new()),
            metrics,
        })
    }

    /// Starts (or refreshes) tracking a peer seen in `sector`.
    pub(crate) fn track(&self, sector: SectorId, addr: PeerAddr) {
        if addr.id == self.me {
            return;
        }
        let mut peers = lock(&self.peers);
        let peer = peers.entry(addr.id).or_insert_with(|| Peer {
            sectors: HashSet::new(),
            addr: addr.clone(),
            conn: None,
            stats: Arc::default(),
            dialing: false,
            last_dial: None,
        });
        peer.sectors.insert(sector);
        peer.addr = addr;
    }

    /// Stops tracking a peer for `sector`; closes the link when no shared Sector remains.
    pub(crate) fn untrack(&self, sector: SectorId, peer: &PeerId) {
        let mut peers = lock(&self.peers);
        let remove = match peers.get_mut(peer) {
            Some(p) => {
                p.sectors.remove(&sector);
                p.sectors.is_empty()
            }
            None => false,
        };
        if remove && let Some(Peer { conn: Some(c), .. }) = peers.remove(peer) {
            c.close(CLOSE_GONE.into(), b"gone");
        }
    }

    /// Drops every peer tracked only through `sector`.
    pub(crate) fn untrack_sector(&self, sector: SectorId) {
        let ids: Vec<PeerId> = lock(&self.peers).keys().copied().collect();
        for id in ids {
            self.untrack(sector, &id);
        }
    }

    fn is_tracked(&self, peer: &PeerId) -> bool {
        lock(&self.peers).contains_key(peer)
    }

    /// Up to `n` tracked peers in `sector` with a live link, for Sector Key bootstrap hints.
    pub(crate) fn connected_in(&self, sector: &SectorId, n: usize) -> Vec<PeerAddr> {
        lock(&self.peers)
            .values()
            .filter(|p| p.conn.is_some() && p.sectors.contains(sector))
            .take(n)
            .map(|p| p.addr.clone())
            .collect()
    }

    fn attach(self: &Arc<Self>, peer: PeerId, conn: Connection) {
        let stats = {
            let mut peers = lock(&self.peers);
            let Some(p) = peers.get_mut(&peer) else {
                conn.close(CLOSE_UNKNOWN.into(), b"unknown");
                return;
            };
            if let Some(old) = p.conn.replace(conn.clone()) {
                old.close(CLOSE_GONE.into(), b"replaced");
            }
            Arc::clone(&p.stats)
        };
        let links = Arc::clone(self);
        tokio::spawn(async move {
            run_pinger(&conn, &stats).await;
            links.detach(&peer, conn.stable_id());
        });
    }

    fn detach(&self, peer: &PeerId, stable_id: usize) {
        if let Some(p) = lock(&self.peers).get_mut(peer)
            && p.conn.as_ref().is_some_and(|c| c.stable_id() == stable_id)
        {
            p.conn = None;
        }
    }

    /// Dials links that are due, refreshes the HUD gauges, and returns per-peer stats.
    pub(crate) fn tick(self: &Arc<Self>) -> Vec<LinkStats> {
        let now = Instant::now();
        let mut out = Vec::new();
        let mut to_dial = Vec::new();
        {
            let mut peers = lock(&self.peers);
            for (id, p) in peers.iter_mut() {
                if p.conn.is_none()
                    && !p.dialing
                    && self.me < *id
                    && p.last_dial
                        .is_none_or(|t| now.duration_since(t) >= DIAL_BACKOFF)
                {
                    p.dialing = true;
                    p.last_dial = Some(now);
                    to_dial.push(p.addr.clone());
                }
                let (rtt_us, loss) = {
                    let s = lock(&p.stats);
                    (s.rtt_us(), s.loss_pct(now))
                };
                let path = p.conn.as_ref().and_then(|c| {
                    c.paths()
                        .iter()
                        .find(|path| path.is_selected())
                        .map(|path| {
                            if path.is_relay() {
                                PathKind::Relay
                            } else {
                                PathKind::Direct
                            }
                        })
                });
                out.push(LinkStats {
                    peer: *id,
                    connected: p.conn.is_some(),
                    rtt_ms: rtt_us.map(|us| us as f32 / 1000.0),
                    loss_pct: loss,
                    path,
                });
            }
        }
        for addr in to_dial {
            self.dial(addr);
        }
        self.update_gauges(&out);
        out.sort_by_key(|l| l.peer);
        out
    }

    fn update_gauges(&self, links: &[LinkStats]) {
        let live = links.iter().filter(|l| l.connected);
        let best_rtt = live
            .clone()
            .filter_map(|l| l.rtt_ms)
            .fold(None, |acc: Option<f32>, r| {
                Some(acc.map_or(r, |a| a.min(r)))
            });
        let worst_loss = live
            .filter_map(|l| l.loss_pct)
            .fold(None, |acc: Option<f32>, r| {
                Some(acc.map_or(r, |a| a.max(r)))
            });
        match best_rtt {
            Some(ms) => self.metrics.rtt_us.set((ms * 1000.0) as u64),
            None => self.metrics.rtt_us.clear(),
        }
        match worst_loss {
            Some(pct) => self.metrics.loss_bp.set((pct * 100.0).round() as u64),
            None => self.metrics.loss_bp.clear(),
        }
    }

    fn dial(self: &Arc<Self>, addr: PeerAddr) {
        let links = Arc::clone(self);
        tokio::spawn(async move {
            let peer = addr.id;
            if let Some(ea) = convert::to_endpoint_addr(&addr) {
                match tokio::time::timeout(DIAL_TIMEOUT, links.endpoint.connect(ea, PING_ALPN))
                    .await
                {
                    Ok(Ok(conn)) => links.attach(peer, conn),
                    Ok(Err(err)) => tracing::debug!(peer = %peer.short(), %err, "ping dial failed"),
                    Err(_) => tracing::debug!(peer = %peer.short(), "ping dial timed out"),
                }
            }
            if let Some(p) = lock(&links.peers).get_mut(&peer) {
                p.dialing = false;
                p.last_dial = Some(Instant::now());
            }
        });
    }
}

async fn run_pinger(conn: &Connection, stats: &Mutex<PingStats>) {
    let mut seq = 0u32;
    let mut iv = tokio::time::interval(PING_INTERVAL);
    iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = iv.tick() => {
                seq = seq.wrapping_add(1);
                if conn.send_datagram(packet(TAG_PING, seq)).is_err() {
                    break;
                }
                lock(stats).on_sent(seq, Instant::now());
            }
            dgram = conn.read_datagram() => {
                let Ok(d) = dgram else { break };
                let Some((tag, s)) = parse(&d) else { continue };
                match tag {
                    TAG_PING => {
                        if conn.send_datagram(packet(TAG_PONG, s)).is_err() {
                            break;
                        }
                    }
                    TAG_PONG => {
                        lock(stats).on_pong(s, Instant::now());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn packet(tag: u8, seq: u32) -> Bytes {
    let mut b = [0u8; PACKET_LEN];
    b[0] = tag;
    b[1..].copy_from_slice(&seq.to_le_bytes());
    Bytes::copy_from_slice(&b)
}

fn parse(d: &[u8]) -> Option<(u8, u32)> {
    if d.len() != PACKET_LEN {
        return None;
    }
    let seq = u32::from_le_bytes(d[1..].try_into().ok()?);
    Some((d[0], seq))
}

/// Router handler for [`PING_ALPN`].
#[derive(Debug, Clone)]
pub(crate) struct PingProtocol(pub(crate) Arc<Links>);

impl ProtocolHandler for PingProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let Some(peer) = convert::peer_id(&conn.remote_id()) else {
            conn.close(CLOSE_UNKNOWN.into(), b"bad id");
            return Ok(());
        };
        if !self.0.is_tracked(&peer) {
            conn.close(CLOSE_UNKNOWN.into(), b"unknown");
            return Ok(());
        }
        self.0.attach(peer, conn);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtt_ewma_and_duplicates() {
        let t0 = Instant::now();
        let mut s = PingStats::default();
        s.on_sent(1, t0);
        assert_eq!(
            s.on_pong(1, t0 + Duration::from_millis(20)),
            Some(Duration::from_millis(20))
        );
        assert_eq!(s.rtt_us(), Some(20_000));
        assert_eq!(s.on_pong(1, t0 + Duration::from_millis(30)), None);
        s.on_sent(2, t0);
        s.on_pong(2, t0 + Duration::from_millis(28));
        assert_eq!(s.rtt_us(), Some(21_000));
    }

    #[test]
    fn loss_counts_only_due_probes() {
        let t0 = Instant::now();
        let mut s = PingStats::default();
        assert_eq!(s.loss_pct(t0), None);
        for seq in 0..4 {
            s.on_sent(seq, t0);
        }
        s.on_pong(0, t0);
        s.on_pong(1, t0);
        assert_eq!(s.loss_pct(t0 + Duration::from_millis(500)), None);
        assert_eq!(s.loss_pct(t0 + LOSS_TIMEOUT), Some(50.0));
    }

    #[test]
    fn window_is_bounded() {
        let t0 = Instant::now();
        let mut s = PingStats::default();
        for seq in 0..(WINDOW as u32 * 2) {
            s.on_sent(seq, t0);
        }
        assert_eq!(s.window.len(), WINDOW);
        assert_eq!(s.on_pong(0, t0), None);
    }

    #[test]
    fn packet_codec() {
        let p = packet(TAG_PING, 0xdead_beef);
        assert_eq!(parse(&p), Some((TAG_PING, 0xdead_beef)));
        assert_eq!(parse(b"xx"), None);
    }
}
