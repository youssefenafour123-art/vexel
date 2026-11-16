//! Pushes 4 Hz core metrics snapshots to the UI as the `telemetry:core` event.
//!
//! The registry is shared with `vxl-net`, which feeds socket byte totals, best RTT and worst loss.

use std::sync::Arc;
use std::time::Instant;

use tauri::{AppHandle, Emitter};
use vxl_metrics::{CoreMetrics, SAMPLE_INTERVAL, Sampler};

/// Event name for core snapshots. Mirrored in `apps/desktop/src/lib/telemetry.ts`.
pub const EVENT_CORE: &str = "telemetry:core";

pub fn spawn(app: AppHandle, metrics: Arc<CoreMetrics>) {
    let spawned = std::thread::Builder::new()
        .name("vxl-telemetry-sampler".into())
        .spawn(move || {
            let mut sampler = Sampler::default();
            let mut last = Instant::now();
            loop {
                std::thread::sleep(SAMPLE_INTERVAL);
                match vxl_platform::memory::working_set_bytes() {
                    Some(b) => metrics.working_set_bytes.set(b),
                    None => metrics.working_set_bytes.clear(),
                }
                let now = Instant::now();
                let snapshot = sampler.sample(&metrics, now - last);
                last = now;
                if app.emit(EVENT_CORE, snapshot).is_err() {
                    // The app is shutting down, so stop sampling.
                    break;
                }
            }
        });

    if let Err(err) = spawned {
        tracing::error!(%err, "failed to spawn telemetry sampler");
    }
}
