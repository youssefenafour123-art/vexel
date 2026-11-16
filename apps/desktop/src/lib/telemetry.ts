import { createSignal, onCleanup, onMount, type Accessor } from "solid-js";
import { listen } from "@tauri-apps/api/event";

/** Mirrors `vxl_metrics::Snapshot` (camelCase). `null` means no reading, rendered as `--`. */
export interface CoreSnapshot {
  coreMb: number | null;
  txKbps: number;
  rxKbps: number;
  rttMs: number | null;
  lossPct: number | null;
}

/** Mirrors `EVENT_CORE` in `src-tauri/src/telemetry.rs`. */
export const EVENT_CORE = "telemetry:core";

export type Status = "ok" | "warn" | "bad" | "idle";

/** Ping status thresholds from BRAND_SYSTEM.md section 1. */
export function pingStatus(ms: number | null): Status {
  if (ms === null) return "idle";
  if (ms < 30) return "ok";
  if (ms <= 80) return "warn";
  return "bad";
}

export function lossStatus(pct: number | null): Status {
  if (pct === null) return "idle";
  if (pct === 0) return "ok";
  if (pct <= 2) return "warn";
  return "bad";
}

export const fmt = {
  ms: (v: number | null) => (v === null ? "--ms" : `${Math.round(v)}ms`),
  kbs: (v: number) => `${v.toFixed(1)} KB/s`,
  mb: (v: number | null) => (v === null ? "-- MB" : `${v.toFixed(1)} MB`),
  loss: (v: number | null) => (v === null ? "--% PKT LOSS" : `${+v.toFixed(1)}% PKT LOSS`),
};

/** Subscribes to the 4 Hz core telemetry event. */
export function useCoreTelemetry(): Accessor<CoreSnapshot | null> {
  const [snap, setSnap] = createSignal<CoreSnapshot | null>(null);
  let unlisten: (() => void) | undefined;
  onCleanup(() => unlisten?.());
  onMount(() => {
    listen<CoreSnapshot>(EVENT_CORE, (e) => setSnap(e.payload))
      .then((u) => (unlisten = u))
      // Outside the Tauri webview (e.g. plain-browser layout checks) there is no IPC; stay at "--".
      .catch(() => undefined);
  });
  return snap;
}
