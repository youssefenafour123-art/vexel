import { createSignal, For, Show } from "solid-js";
import { activeSector, errText, flash, net, setDisplayName, type Member } from "../lib/net";
import { fmt, pingStatus } from "../lib/telemetry";
import "./ChannelPanel.css";

/** Channel list + members of the active Sector, with the local profile pinned at the bottom. */
export function ChannelPanel() {
  const sector = activeSector;
  const onlineCount = () => sector()?.members.filter((m) => m.online).length ?? 0;

  return (
    <aside class="panel">
      <div class="panel-header">
        <Show when={sector()} fallback={<span class="label muted">NO SECTOR</span>}>
          {(s) => (
            <>
              <span class="panel-title" title={s().name}>{s().name}</span>
              <Show when={s().isOwner}>
                <span class="chip label">OWNER</span>
              </Show>
            </>
          )}
        </Show>
      </div>

      <div class="panel-body">
        <Show when={sector()} fallback={<p class="label muted">COMMS CHANNELS // --</p>}>
          <section>
            <p class="label muted section-label">CHANNELS</p>
            <div class="row" aria-disabled="true" title="Text comms arrive in Phase 2">
              <span class="row-glyph muted">#</span>
              <span class="row-name">general</span>
              <span class="label muted">P2</span>
            </div>
            <div class="row" aria-disabled="true" title="Comms Channels arrive in Phase 3">
              <svg class="row-glyph muted" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                <path d="M2 4.5h2l3-2.5v8L4 7.5H2z M9 4a2.5 2.5 0 0 1 0 4" stroke="currentColor" fill="none" />
              </svg>
              <span class="row-name">Comms 1</span>
              <span class="label muted">P3</span>
            </div>
          </section>

          <section>
            <p class="label muted section-label">
              MEMBERS // {onlineCount()} ONLINE
            </p>
            <For each={sector()?.members ?? []}>{(m) => <MemberRow member={m} />}</For>
          </section>
        </Show>
      </div>

      <ProfileFooter />
    </aside>
  );
}

function MemberRow(props: { member: Member }) {
  const link = () => net.links[props.member.peer];
  const rtt = () => (props.member.online && link()?.connected ? link()?.rttMs ?? null : null);

  return (
    <div class="row member" classList={{ offline: !props.member.online }}>
      <span class="presence" data-online={props.member.online} aria-hidden="true" />
      <span class="row-name" title={`${props.member.displayName} // ${props.member.shortId}`}>
        {props.member.displayName}
      </span>
      <Show when={props.member.isSelf}>
        <span class="label muted">YOU</span>
      </Show>
      <Show when={!props.member.isSelf && props.member.online}>
        <span class="label member-ping" data-status={pingStatus(rtt())}>
          {fmt.ms(rtt())}
        </span>
      </Show>
    </div>
  );
}

function ProfileFooter() {
  const [editing, setEditing] = createSignal(false);
  let input: HTMLInputElement | undefined;

  const save = async () => {
    const name = input?.value ?? "";
    if (name.trim() === net.profile?.displayName) return setEditing(false);
    try {
      await setDisplayName(name);
      setEditing(false);
      flash("Callsign updated.");
    } catch (e) {
      flash(errText(e));
    }
  };

  return (
    <div class="panel-footer">
      <Show
        when={editing()}
        fallback={
          <button class="profile" onClick={() => setEditing(true)} title="Edit callsign" disabled={!net.profile}>
            <span class="profile-name">{net.profile?.displayName ?? "--"}</span>
            <span class="label muted">{net.profile?.shortId ?? "--------"}</span>
          </button>
        }
      >
        <input
          ref={(el) => {
            input = el;
            queueMicrotask(() => el.select());
          }}
          class="input"
          maxLength={32}
          value={net.profile?.displayName ?? ""}
          aria-label="Callsign"
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
            if (e.key === "Escape") setEditing(false);
          }}
          onBlur={() => void save()}
        />
      </Show>
    </div>
  );
}
