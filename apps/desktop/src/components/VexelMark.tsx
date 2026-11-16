import type { JSX } from "solid-js";

/**
 * Canonical VEXEL mark (BRAND_SYSTEM.md section 6). `live` recolors the two volt elements
 * (right chevron and apex notch), which form the live-state channel.
 */
export function VexelMark(props: { size?: number; live?: string; plate?: boolean }): JSX.Element {
  const size = () => props.size ?? 24;
  const live = () => props.live ?? "var(--vxl-volt)";
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 512 512"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="VEXEL"
      role="img"
    >
      {props.plate !== false && (
        <>
          <rect width="512" height="512" rx="96" fill="var(--vxl-bg)" />
          {size() > 20 && (
            <rect x="6" y="6" width="500" height="500" rx="90" stroke="var(--vxl-border-icon)" stroke-width="4" />
          )}
        </>
      )}
      <path d="M96 128H176L256 324L216 400L96 128Z" fill="var(--vxl-text)" />
      <path d="M416 128H336L240 362L260 400L416 128Z" fill={live()} />
      <polygon points="236,128 276,128 256,172" fill={live()} />
    </svg>
  );
}
