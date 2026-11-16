import { createSignal, onCleanup, onMount } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/* ---- Wire types: mirror `src-tauri/src/net.rs`, `vxl_core::MemberView`, `vxl_net::LinkStats` ---- */

export interface Profile {
  peerId: string;
  shortId: string;
  displayName: string;
}

export interface Member {
  peer: string;
  shortId: string;
  displayName: string;
  role: "owner" | "member";
  online: boolean;
  isSelf: boolean;
}

export interface SectorView {
  id: string;
  name: string;
  isOwner: boolean;
  members: Member[];
}

export interface KeyPreview {
  sectorId: string;
  sectorName: string;
  ownerShort: string;
  expiresAtMs: number | null;
  expired: boolean;
  alreadyJoined: boolean;
}

export interface LinkStats {
  peer: string;
  connected: boolean;
  rttMs: number | null;
  lossPct: number | null;
  path: "direct" | "relay" | null;
}

export interface OnlineView {
  relay: string | null;
  direct: number;
}

/* Event names: mirror the constants in src-tauri. */
export const EVENT_ROSTER = "net:roster";
export const EVENT_LINKS = "net:links";
export const EVENT_ONLINE = "net:online";
export const EVENT_SECTOR_KEY = "deeplink:sector-key";

/* ---- App-wide state ---- */

interface NetState {
  profile: Profile | null;
  sectors: SectorView[];
  /** Per-peer link stats, keyed by peer id. */
  links: Record<string, LinkStats>;
  online: OnlineView | null;
}

const [state, setState] = createStore<NetState>({
  profile: null,
  sectors: [],
  links: {},
  online: null,
});
const [activeId, setActiveId] = createSignal<string | null>(null);
/** A Sector Key waiting for the user to confirm (from a vxl:// link). */
const [pendingKey, setPendingKey] = createSignal<string | null>(null);
/** One-line pit-wall status ("Sector Key copied."). */
const [notice, setNotice] = createSignal<string | null>(null);

export { state as net, activeId, setActiveId, pendingKey, setPendingKey, notice };

let noticeTimer: ReturnType<typeof setTimeout> | undefined;
export function flash(msg: string) {
  setNotice(msg);
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => setNotice(null), 2400);
}

export const activeSector = () => state.sectors.find((s) => s.id === activeId()) ?? null;

/** Errors from commands arrive as strings; anything else becomes a generic cause. */
export function errText(e: unknown): string {
  return (typeof e === "string" ? e : "LINK ERROR // UNKNOWN").toUpperCase();
}

function upsertSector(view: SectorView) {
  const i = state.sectors.findIndex((s) => s.id === view.id);
  if (i === -1) setState("sectors", state.sectors.length, view);
  else setState("sectors", i, "name", view.name);
}

/* ---- Commands ---- */

export async function refresh() {
  const [profile, sectors, online] = await Promise.all([
    invoke<Profile>("profile_get"),
    invoke<SectorView[]>("sectors_list"),
    invoke<OnlineView | null>("link_status"),
  ]);
  setState({ profile, sectors, online });
  if (!activeId() && sectors.length > 0) setActiveId(sectors[0].id);
}

export async function createSector(name: string) {
  const view = await invoke<SectorView>("sector_create", { name });
  upsertSector(view);
  setActiveId(view.id);
  return view;
}

export const previewKey = (key: string) => invoke<KeyPreview>("sector_key_preview", { key });

export async function joinSector(key: string) {
  const view = await invoke<SectorView>("sector_join", { key });
  upsertSector(view);
  setActiveId(view.id);
  return view;
}

export async function copySectorKey(sectorId: string) {
  const key = await invoke<string>("sector_key", { sectorId });
  await navigator.clipboard.writeText(key);
  flash("Sector Key copied.");
  return key;
}

export async function leaveSector(sectorId: string) {
  await invoke("sector_leave", { sectorId });
  setState(
    "sectors",
    state.sectors.filter((s) => s.id !== sectorId),
  );
  if (activeId() === sectorId) setActiveId(state.sectors[0]?.id ?? null);
}

export async function setDisplayName(name: string) {
  const profile = await invoke<Profile>("profile_set_name", { name });
  setState("profile", profile);
}

/* ---- Event wiring (call once, from App) ---- */

/** Polls until the node is up (it starts asynchronously), then subscribes to net events. */
export function useNet() {
  const unlisteners: UnlistenFn[] = [];
  let retry: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => {
    clearTimeout(retry);
    unlisteners.forEach((u) => u());
  });

  onMount(async () => {
    try {
      unlisteners.push(
        await listen<{ sectorId: string; members: Member[] }>(EVENT_ROSTER, (e) => {
          const i = state.sectors.findIndex((s) => s.id === e.payload.sectorId);
          if (i !== -1) setState("sectors", i, "members", reconcile(e.payload.members, { key: "peer" }));
        }),
        await listen<LinkStats[]>(EVENT_LINKS, (e) => {
          const next: Record<string, LinkStats> = {};
          for (const l of e.payload) next[l.peer] = l;
          setState("links", reconcile(next));
        }),
        await listen<OnlineView>(EVENT_ONLINE, (e) => setState("online", e.payload)),
        await listen<string>(EVENT_SECTOR_KEY, (e) => setPendingKey(e.payload)),
      );
    } catch {
      // Plain browser (no Tauri IPC): the UI stays in its offline state.
      return;
    }

    const boot = async () => {
      try {
        await refresh();
        const key = await invoke<string | null>("deeplink_take");
        if (key) setPendingKey(key);
      } catch {
        retry = setTimeout(boot, 500);
      }
    };
    void boot();
  });
}

/* ---- Formatting ---- */

/** `vxl://s/abcd…wxyz` (middle truncated, per BRAND_SYSTEM.md section 4). */
export function truncateKey(key: string, keep = 10): string {
  const prefix = "vxl://s/";
  const body = key.startsWith(prefix) ? key.slice(prefix.length) : key;
  if (body.length <= keep * 2 + 1) return key;
  return `${prefix}${body.slice(0, keep)}…${body.slice(-keep)}`;
}

/** Initials for a Sector squircle: first letters of up to two words. */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const chars = words.length > 1 ? [words[0][0], words[1][0]] : [...(words[0] ?? "?")].slice(0, 2);
  return chars.join("").toUpperCase();
}

export function expiryLabel(ms: number | null): string {
  if (ms === null) return "NO EXPIRY";
  const hours = Math.max(0, Math.round((ms - Date.now()) / 3_600_000));
  return hours >= 48 ? `EXPIRES IN ${Math.round(hours / 24)}D` : `EXPIRES IN ${hours}H`;
}
