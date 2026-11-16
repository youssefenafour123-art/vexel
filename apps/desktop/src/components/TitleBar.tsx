import { getCurrentWindow } from "@tauri-apps/api/window";
import { VexelMark } from "./VexelMark";
import "./TitleBar.css";

/** Custom frameless title bar: brand wordmark, OS tag and window controls. */
export function TitleBar() {
  return (
    <header class="titlebar" data-tauri-drag-region>
      <div class="titlebar-brand" data-tauri-drag-region>
        <VexelMark size={16} plate={false} />
        <span class="wordmark" data-tauri-drag-region>VEXEL</span>
        <span class="label muted" data-tauri-drag-region>// SUB-MILLISECOND COMMS</span>
      </div>
      <div class="titlebar-controls">
        <button class="wc" aria-label="Minimize" onClick={() => getCurrentWindow().minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1 5h8" stroke="currentColor" /></svg>
        </button>
        <button class="wc" aria-label="Maximize" onClick={() => getCurrentWindow().toggleMaximize()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x="1.5" y="1.5" width="7" height="7" stroke="currentColor" fill="none" /></svg>
        </button>
        <button class="wc wc-close" aria-label="Close" onClick={() => getCurrentWindow().close()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="currentColor" /></svg>
        </button>
      </div>
    </header>
  );
}
