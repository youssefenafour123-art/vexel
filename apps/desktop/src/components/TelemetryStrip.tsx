import { Show } from "solid-js";
import { net, notice } from "../lib/net";
import { fmt, lossStatus, pingStatus, useCoreTelemetry } from "../lib/telemetry";
import "./TelemetryStrip.css";

/**
 * Always-visible bottom telemetry strip. Every value is a real reading; missing readings show `--`.
 * CORE is the Rust core's working set only (WebView2 processes are excluded).
 */
export function TelemetryStrip(props: { version: string }) {
  const snap = useCoreTelemetry();
  const rtt = () => snap()?.rttMs ?? null;
  const loss = () => snap()?.lossPct ?? null;
  const linked = () => rtt() !== null;
  // UP: at least one peer link measured. STANDBY: node online, no peer yet. OFFLINE: node not up.
  const linkState = () => (linked() ? "UP" : net.online ? "STANDBY" : "OFFLINE");

  return (
    <footer class="strip label">
      <div class="strip-group">
        <span class="seg" data-status={linked() ? "ok" : "idle"} title={net.online?.relay ?? "No home relay"}>
          LINK // {linkState()}
        </span>
        <span class="seg" data-status={pingStatus(rtt())}>{fmt.ms(rtt())}</span>
        <span class="seg" data-status={lossStatus(loss())}>{fmt.loss(loss())}</span>
        <span class="seg muted">↑ {fmt.kbs(snap()?.txKbps ?? 0)}</span>
        <span class="seg muted">↓ {fmt.kbs(snap()?.rxKbps ?? 0)}</span>
      </div>
      <div class="strip-group">
        <Show when={notice()}>
          <span class="seg notice" role="status">{notice()}</span>
        </Show>
        <span class="seg" title="Rust core working set (WebView2 excluded)">
          <span class="muted">CORE</span> {fmt.mb(snap()?.coreMb ?? null)}
        </span>
        <span class="seg muted">v{props.version}</span>
      </div>
    </footer>
  );
}
