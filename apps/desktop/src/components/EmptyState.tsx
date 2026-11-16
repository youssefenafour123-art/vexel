import { VexelMark } from "./VexelMark";
import "./EmptyState.css";

/** Main-area empty state shown until the user links a Sector. */
export function EmptyState(props: { tagline: string }) {
  return (
    <main class="empty">
      <VexelMark size={96} />
      <h1 class="empty-title label">NO SECTOR LINKED</h1>
      <p class="empty-tagline muted">{props.tagline}</p>
      <div class="empty-actions">
        <button class="btn btn-primary" disabled title="Arrives in Phase 1">
          Create Sector
        </button>
        <button class="btn" disabled title="Arrives in Phase 1">
          Paste Sector Key
        </button>
      </div>
      <p class="label muted">
        <span class="chip">PHASE 0</span> SECTOR LINKING ARRIVES IN PHASE 1
      </p>
    </main>
  );
}
