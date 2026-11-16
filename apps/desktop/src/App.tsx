import { createResource } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { TitleBar } from "./components/TitleBar";
import { SectorRail } from "./components/SectorRail";
import { ChannelPanel } from "./components/ChannelPanel";
import { EmptyState } from "./components/EmptyState";
import { TelemetryStrip } from "./components/TelemetryStrip";
import "./App.css";

/** Mirrors `AppInfo` in `src-tauri/src/lib.rs`. */
interface AppInfo {
  product: string;
  osTag: string;
  tagline: string;
  version: string;
}

export default function App() {
  const [info] = createResource(() => invoke<AppInfo>("app_info"));

  return (
    <div class="shell">
      <TitleBar />
      <div class="shell-body">
        <SectorRail />
        <ChannelPanel />
        <EmptyState tagline={info()?.tagline ?? ""} />
      </div>
      <TelemetryStrip version={info()?.version ?? "-"} />
    </div>
  );
}
