import { For } from "solid-js";
import { activeId, initials, net, setActiveId } from "../lib/net";
import "./SectorRail.css";

/** Far-left Sector rail: one squircle per linked Sector, plus the add slot. */
export function SectorRail(props: { onAdd: () => void }) {
  return (
    <nav class="rail" aria-label="Sectors">
      <For each={net.sectors}>
        {(s) => (
          <button
            class="rail-sector"
            classList={{ active: activeId() === s.id }}
            title={s.name}
            aria-label={s.name}
            aria-current={activeId() === s.id ? "true" : undefined}
            onClick={() => setActiveId(s.id)}
          >
            <span class="rail-bar" aria-hidden="true" />
            <span class="label">{initials(s.name)}</span>
          </button>
        )}
      </For>
      <button class="rail-slot" title="Create or join a Sector" aria-label="Add Sector" onClick={() => props.onAdd()}>
        <svg width="16" height="16" viewBox="0 0 16 16">
          <path d="M8 2v12M2 8h12" stroke="currentColor" stroke-width="1.5" />
        </svg>
      </button>
    </nav>
  );
}
