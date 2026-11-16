import { createEffect, createSignal, on, onCleanup, onMount, Show } from "solid-js";
import { createSector, errText, expiryLabel, flash, joinSector, previewKey, type KeyPreview } from "../lib/net";
import "./LinkDialog.css";

export type LinkMode = "create" | "join";

/** Modal for creating a Sector or joining one from a Sector Key. Nothing joins without a click. */
export function LinkDialog(props: { mode: LinkMode; initialKey?: string; onClose: () => void }) {
  const [mode, setMode] = createSignal<LinkMode>(props.mode);
  const [name, setName] = createSignal("");
  const [key, setKey] = createSignal(props.initialKey ?? "");
  const [preview, setPreview] = createSignal<KeyPreview | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));

  // Validate the pasted key as it changes (decode + signature check happen in Rust).
  let seq = 0;
  createEffect(
    on([key, mode], async ([k, m]) => {
      setPreview(null);
      setError(null);
      if (m !== "join" || k.trim() === "") return;
      const mine = ++seq;
      try {
        const p = await previewKey(k);
        if (mine === seq) setPreview(p);
      } catch (e) {
        if (mine === seq) setError(errText(e));
      }
    }),
  );

  const canJoin = () => {
    const p = preview();
    return !busy() && p !== null && !p.expired && !p.alreadyJoined;
  };

  const submit = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (mode() === "create") {
        const s = await createSector(name());
        flash(`Sector ${s.name} online.`);
      } else {
        const s = await joinSector(key());
        flash(`Linking to ${s.name}.`);
      }
      props.onClose();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="modal-layer" role="presentation">
      <div class="modal-backdrop" onClick={() => props.onClose()} />
      <form class="modal" role="dialog" aria-modal="true" aria-label="Link a Sector" onSubmit={submit}>
        <div class="tabs" role="tablist">
          <button type="button" role="tab" class="tab label" aria-selected={mode() === "create"} onClick={() => setMode("create")}>
            CREATE SECTOR
          </button>
          <button type="button" role="tab" class="tab label" aria-selected={mode() === "join"} onClick={() => setMode("join")}>
            PASTE SECTOR KEY
          </button>
        </div>

        <div class="modal-body">
          <Show
            when={mode() === "create"}
            fallback={
              <>
                <label class="label muted" for="vxl-key">SECTOR KEY</label>
                <textarea
                  id="vxl-key"
                  class="input key-input mono"
                  rows={4}
                  spellcheck={false}
                  placeholder="vxl://s/…"
                  value={key()}
                  onInput={(e) => setKey(e.currentTarget.value)}
                  ref={(el) => queueMicrotask(() => el.focus())}
                />
                <Show when={preview()}>
                  {(p) => (
                    <p class="label preview" data-status={p().expired || p().alreadyJoined ? "bad" : "ok"}>
                      {p().sectorName} // OWNER {p().ownerShort} //{" "}
                      {p().alreadyJoined ? "ALREADY LINKED" : p().expired ? "KEY EXPIRED" : expiryLabel(p().expiresAtMs)}
                    </p>
                  )}
                </Show>
              </>
            }
          >
            <label class="label muted" for="vxl-name">SECTOR NAME</label>
            <input
              id="vxl-name"
              class="input"
              maxLength={48}
              placeholder="Night Ops"
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
              ref={(el) => queueMicrotask(() => el.focus())}
            />
            <p class="label muted">YOU OWN IT // INVITES ARE SIGNED WITH YOUR KEY</p>
          </Show>

          <Show when={error()}>
            <p class="label preview" data-status="bad">{error()}</p>
          </Show>
        </div>

        <div class="modal-foot">
          <button type="button" class="btn" onClick={() => props.onClose()}>
            Cancel
          </button>
          <Show
            when={mode() === "create"}
            fallback={
              <button type="submit" class="btn btn-primary" disabled={!canJoin()}>
                Join Sector
              </button>
            }
          >
            <button type="submit" class="btn btn-primary" disabled={busy() || name().trim() === ""}>
              Create Sector
            </button>
          </Show>
        </div>
      </form>
    </div>
  );
}
