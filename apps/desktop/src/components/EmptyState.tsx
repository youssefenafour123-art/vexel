import { VexelMark } from "./VexelMark";
import type { LinkMode } from "./LinkDialog";
import "./EmptyState.css";

/** Main-area empty state shown until the user links a Sector. */
export function EmptyState(props: { tagline: string; ready: boolean; onLink: (mode: LinkMode) => void }) {
  return (
    <main class="empty">
      <VexelMark size={96} />
      <h1 class="empty-title label">NO SECTOR LINKED</h1>
      <p class="empty-tagline muted">{props.tagline}</p>
      <div class="empty-actions">
        <button class="btn btn-primary" disabled={!props.ready} onClick={() => props.onLink("create")}>
          Create Sector
        </button>
        <button class="btn" disabled={!props.ready} onClick={() => props.onLink("join")}>
          Paste Sector Key
        </button>
      </div>
      <p class="label muted">{props.ready ? "P2P // NO CENTRAL SERVICE // KEYS ARE SIGNED" : "LINK NODE STARTING…"}</p>
    </main>
  );
}
