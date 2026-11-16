import { createEffect, createResource, createSignal, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { TitleBar } from "./components/TitleBar";
import { SectorRail } from "./components/SectorRail";
import { ChannelPanel } from "./components/ChannelPanel";
import { EmptyState } from "./components/EmptyState";
import { SectorMain } from "./components/SectorMain";
import { LinkDialog, type LinkMode } from "./components/LinkDialog";
import { TelemetryStrip } from "./components/TelemetryStrip";
import { activeSector, net, pendingKey, setPendingKey, useNet } from "./lib/net";
import "./App.css";

/** Mirrors `AppInfo` in `src-tauri/src/lib.rs`. */
interface AppInfo {
  product: string;
  osTag: string;
  tagline: string;
  version: string;
}

interface DialogState {
  mode: LinkMode;
  key?: string;
}

export default function App() {
  const [info] = createResource(() => invoke<AppInfo>("app_info").catch(() => undefined));
  const [dialog, setDialog] = createSignal<DialogState | null>(null);
  useNet();

  // A vxl:// link opens the join dialog prefilled; the user still confirms.
  createEffect(() => {
    const key = pendingKey();
    if (key && net.profile) {
      setDialog({ mode: "join", key });
      setPendingKey(null);
    }
  });

  return (
    <div class="shell">
      <TitleBar />
      <div class="shell-body">
        <SectorRail onAdd={() => setDialog({ mode: "create" })} />
        <ChannelPanel />
        <Show
          when={activeSector()}
          keyed
          fallback={
            <EmptyState
              tagline={info()?.tagline ?? ""}
              ready={net.profile !== null}
              onLink={(mode) => setDialog({ mode })}
            />
          }
        >
          {(s) => <SectorMain sector={s} />}
        </Show>
      </div>
      <TelemetryStrip version={info()?.version ?? "-"} />
      <Show when={dialog()} keyed>
        {(d) => <LinkDialog mode={d.mode} initialKey={d.key} onClose={() => setDialog(null)} />}
      </Show>
    </div>
  );
}
