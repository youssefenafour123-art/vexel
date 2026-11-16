import "./ChannelPanel.css";

/** Channel list panel. Shows Comms Channels once a Sector is linked (Phase 1+). */
export function ChannelPanel() {
  return (
    <aside class="panel">
      <div class="panel-header">
        <span class="label muted">NO SECTOR</span>
      </div>
      <div class="panel-body">
        <p class="label muted">COMMS CHANNELS // --</p>
      </div>
    </aside>
  );
}
