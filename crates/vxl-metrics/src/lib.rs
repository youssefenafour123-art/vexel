//! # vxl-metrics
//!
//! Lock-free telemetry primitives. Hot paths (audio, network) only touch atomics: writes are a single
//! relaxed `fetch_add` or `store`, with no allocation and no locks. A sampler thread reads them at
//! 4 Hz and turns them into a serializable [`Snapshot`] for the UI.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

/// HUD sampling interval (4 Hz). Per `CLAUDE.md` section 4, metrics must not reach the UI more often than this.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// Monotonic counter, e.g. bytes sent. Safe to bump from real-time threads.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    #[inline]
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Last-value gauge, e.g. RTT in microseconds or working set in bytes.
/// `u64::MAX` means "no reading yet", which the UI shows as `--`.
#[derive(Debug)]
pub struct Gauge(AtomicU64);

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

impl Gauge {
    pub const UNSET: u64 = u64::MAX;

    pub const fn new() -> Self {
        Self(AtomicU64::new(Self::UNSET))
    }

    #[inline]
    pub fn set(&self, v: u64) {
        self.0.store(v, Ordering::Relaxed);
    }

    #[inline]
    pub fn clear(&self) {
        self.0.store(Self::UNSET, Ordering::Relaxed);
    }

    #[inline]
    pub fn get(&self) -> Option<u64> {
        match self.0.load(Ordering::Relaxed) {
            Self::UNSET => None,
            v => Some(v),
        }
    }
}

/// Turns a [`Counter`] into a rate (units per second) between successive samples.
#[derive(Debug, Default)]
pub struct RateSampler {
    last: u64,
}

impl RateSampler {
    /// Returns the rate since the previous call, given the elapsed interval.
    pub fn sample(&mut self, counter: &Counter, elapsed: Duration) -> f64 {
        let now = counter.get();
        let delta = now.wrapping_sub(self.last);
        self.last = now;
        let secs = elapsed.as_secs_f64();
        if secs <= 0.0 {
            0.0
        } else {
            delta as f64 / secs
        }
    }
}

/// Process-wide core telemetry registry.
#[derive(Debug, Default)]
pub struct CoreMetrics {
    /// Working set of the Rust core process, in bytes. This excludes WebView2 child processes.
    pub working_set_bytes: Gauge,
    /// Bytes sent on all media and control streams.
    pub tx_bytes: Counter,
    /// Bytes received on all media and control streams.
    pub rx_bytes: Counter,
    /// Best peer RTT in microseconds (Phase 1+).
    pub rtt_us: Gauge,
    /// Packet loss in basis points, where 100 = 1% (Phase 1+).
    pub loss_bp: Gauge,
}

/// Serializable 4 Hz snapshot pushed to the UI. `None` means "no reading", which the UI renders as `--`.
/// This follows the brand rule that metrics are never faked.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub core_mb: Option<f64>,
    pub tx_kbps: f64,
    pub rx_kbps: f64,
    pub rtt_ms: Option<u32>,
    pub loss_pct: Option<f32>,
}

/// Holds the rate state needed to turn [`CoreMetrics`] into [`Snapshot`]s.
#[derive(Debug, Default)]
pub struct Sampler {
    tx: RateSampler,
    rx: RateSampler,
}

impl Sampler {
    pub fn sample(&mut self, m: &CoreMetrics, elapsed: Duration) -> Snapshot {
        Snapshot {
            core_mb: m
                .working_set_bytes
                .get()
                .map(|b| b as f64 / (1024.0 * 1024.0)),
            tx_kbps: self.tx.sample(&m.tx_bytes, elapsed) / 1024.0,
            rx_kbps: self.rx.sample(&m.rx_bytes, elapsed) / 1024.0,
            rtt_ms: m
                .rtt_us
                .get()
                .map(|us| u32::try_from(us / 1000).unwrap_or(u32::MAX)),
            loss_pct: m.loss_bp.get().map(|bp| bp as f32 / 100.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_unset_until_written() {
        let g = Gauge::new();
        assert_eq!(g.get(), None);
        g.set(42);
        assert_eq!(g.get(), Some(42));
        g.clear();
        assert_eq!(g.get(), None);
    }

    #[test]
    fn rate_sampler_computes_per_second() {
        let c = Counter::new();
        let mut r = RateSampler::default();
        c.add(1000);
        assert_eq!(r.sample(&c, Duration::from_millis(250)), 4000.0);
        assert_eq!(r.sample(&c, Duration::from_millis(250)), 0.0);
    }

    #[test]
    fn snapshot_reports_none_for_missing_readings() {
        let m = CoreMetrics::default();
        let mut s = Sampler::default();
        m.working_set_bytes.set(32 * 1024 * 1024);
        let snap = s.sample(&m, SAMPLE_INTERVAL);
        assert_eq!(snap.core_mb, Some(32.0));
        assert_eq!(snap.rtt_ms, None);
        assert_eq!(snap.loss_pct, None);
        assert_eq!(snap.tx_kbps, 0.0);
    }
}
