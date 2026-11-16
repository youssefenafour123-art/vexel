import "./SectorRail.css";

/** Far-left Sector rail. Phase 1 fills it with joined Sectors; for now it only shows the add slot. */
export function SectorRail() {
  return (
    <nav class="rail" aria-label="Sectors">
      <button class="rail-slot" disabled title="Create or join a Sector (Phase 1)" aria-label="Add Sector">
        <svg width="16" height="16" viewBox="0 0 16 16">
          <path d="M8 2v12M2 8h12" stroke="currentColor" stroke-width="1.5" />
        </svg>
      </button>
    </nav>
  );
}
