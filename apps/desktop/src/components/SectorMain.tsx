import { createSignal, For, Show } from "solid-js";
import { copySectorKey, errText, flash, leaveSector, net, truncateKey, type SectorView } from "../lib/net";
import { fmt, lossStatus, pingStatus } from "../lib/telemetry";
import "./SectorMain.css";

/** Main area for a linked Sector: Sector Key, live link table, leave control. */
export function SectorMain(props: { sector: SectorView }) {
  const [key, setKey] = createSignal<string | null>(null);
  const [confirmLeave, setConfirmLeave] = createSignal(false);
  const peers = () => props.sector.members.filter((m) => !m.isSelf);

  const copy = async () => {
    try {
      setKey(await copySectorKey(props.sector.id));
    } catch (e) {
      flash(errText(e));
    }
  };

  const leave = async () => {
    try {
      await leaveSector(props.sector.id);
      flash("Sector unlinked.");
    } catch (e) {
      flash(errText(e));
    }
  };

  return (
    <main class="sector-main">
      <header class="sector-head">
        <p class="label muted">SECTOR // {props.sector.id.slice(0, 8)}</p>
        <h1 class="sector-name">{props.sector.name}</h1>
      </header>

      <section class="card">
        <div class="card-head">
          <span class="label">SECTOR KEY</span>
          <span class="label muted">
            {props.sector.isOwner ? "OWNER-SIGNED // 7D" : "RE-SHARES YOUR INVITE"}
          </span>
        </div>
        <div class="key-row">
          <code class="key mono muted" title={key() ?? undefined}>
            {key() ? truncateKey(key()!) : "vxl://s/…"}
          </code>
          <button class="btn btn-primary" onClick={() => void copy()}>
            Copy Sector Key
          </button>
        </div>
      </section>

      <section class="card">
        <div class="card-head">
          <span class="label">LINK TABLE</span>
          <span class="label muted">{peers().filter((m) => m.online).length} PEERS ONLINE</span>
        </div>
        <Show when={peers().length > 0} fallback={<p class="label muted empty-note">NO PEERS YET // SHARE THE SECTOR KEY</p>}>
          <table class="links">
            <thead>
              <tr class="label muted">
                <th>PEER</th>
                <th>PING</th>
                <th>LOSS</th>
                <th>PATH</th>
              </tr>
            </thead>
            <tbody>
              <For each={peers()}>
                {(m) => {
                  const link = () => (m.online ? net.links[m.peer] : undefined);
                  const rtt = () => (link()?.connected ? link()?.rttMs ?? null : null);
                  const loss = () => (link()?.connected ? link()?.lossPct ?? null : null);
                  const path = () =>
                    !m.online ? "OFFLINE" : !link()?.connected ? "LINKING" : link()?.path === "relay" ? "RELAY" : "DIRECT";
                  return (
                    <tr>
                      <td>
                        <span class="peer-name">{m.displayName}</span>
                        <span class="label muted"> // {m.shortId}</span>
                        <Show when={m.role === "owner"}>
                          <span class="label muted"> // OWNER</span>
                        </Show>
                      </td>
                      <td class="label" data-status={pingStatus(rtt())}>{fmt.ms(rtt())}</td>
                      <td class="label" data-status={lossStatus(loss())}>
                        {loss() === null ? "--%" : `${+loss()!.toFixed(1)}%`}
                      </td>
                      <td class="label" data-status={path() === "DIRECT" ? "ok" : path() === "RELAY" ? "warn" : "idle"}>
                        {path()}
                      </td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>
        </Show>
      </section>

      <p class="label muted">TEXT COMMS // PHASE 2 · COMMS CHANNELS // PHASE 3</p>

      <div class="sector-foot">
        <Show
          when={confirmLeave()}
          fallback={
            <button class="btn btn-danger" onClick={() => setConfirmLeave(true)}>
              Leave Sector
            </button>
          }
        >
          <span class="label muted">
            {props.sector.isOwner ? "YOU OWN THIS SECTOR // REJOIN NEEDS A KEY FROM A MEMBER" : "REJOIN NEEDS A NEW SECTOR KEY"}
          </span>
          <button class="btn btn-danger" onClick={() => void leave()}>
            Confirm Leave
          </button>
          <button class="btn" onClick={() => setConfirmLeave(false)}>
            Cancel
          </button>
        </Show>
      </div>
    </main>
  );
}
