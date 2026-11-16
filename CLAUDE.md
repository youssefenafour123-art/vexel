# CLAUDE.md — VEXEL (`VXL`)

> **VEXEL // SUB-MILLISECOND COMMS** — *"Sub-Millisecond Comms. Zero Engine Drag."*
> A $0-budget, peer-to-peer Discord alternative for competitive gamers: voice, text, screen share and a
> serverless music bot, built so that every KB of RAM, every packet and every µs of audio buffer can be seen, measured and controlled by the user.

Read this file and `BRAND_SYSTEM.md` at the start of every session. The full roadmap is in the
**Phase Status** section at the bottom; update it when a phase changes state.

---

## 1. Locked decisions (do not re-litigate without the user)

| Decision | Choice |
|---|---|
| Shell | **Tauri 2** (Rust core + WebView2 UI) |
| UI | **SolidJS + TypeScript + Vite**, plain CSS custom properties (no Tailwind, no CSS-in-JS runtime) |
| Platform | **Windows-only v1**. All OS-specific code sits behind traits in `vxl-platform` so other platforms can be added later |
| P2P | **iroh** (QUIC, hole punching, relay fallback) + **iroh-gossip**. No central server. n0 public relays by default, relay URL configurable |
| Voice topology | **Full mesh, ≤ ~10 peers per Comms Channel**, Opus over QUIC datagrams |
| Control-plane wire format | **MessagePack** (`rmp-serde`). Media packets use a custom fixed binary header |
| Storage | **SQLite** via `rusqlite` (bundled) |
| Aux Channel | Discord-style `/play` bot. Managed **yt-dlp** binary, **zero-reencode Opus passthrough**, DJ Node failover |
| Package manager | **pnpm** (via corepack) |
| Budget | **$0**: no paid services, no paid certificates, CI on free GitHub Actions |

---

## 2. Brand vocabulary (mandatory in UI strings, type names, module names and docs)

| Generic term | VEXEL term | Code identifier |
|---|---|---|
| Server / room | **Sector** | `Sector`, `SectorId` |
| Invite link | **Sector Key** (`vxl://`) | `SectorKey` |
| Voice channel | **Comms Channel** | `CommsChannel` |
| Screen share / Go Live | **Direct Telemetry** | `telemetry`, `DirectTelemetry` |
| Music bot | **Aux Channel** / `AUX // BOT` | `aux`, `AuxBot` |
| Bot host peer | **DJ Node** | `DjNode` |
| Minimized game mode | **Zero-Drag Mode** | `ZeroDrag` |
| Silence suppression / DTX | **Zero-Floor Gate** | `ZeroFloorGate` |

**Never** use "server", "guild", "voice channel", "go live", "music bot" or "stream" (meaning screen share) in UI copy or public type names.
- Protocol: `vxl://` (Sector Keys). File extension: `.vxl` (theme and soundboard packs).

---

## 3. Workspace layout

```
Vexel/
├─ Cargo.toml            # [workspace]; shared deps in [workspace.dependencies]
├─ crates/
│  ├─ vxl-core/          # identity (ed25519), Sector/Channel/Member model, SectorKey codec, MessagePack wire types. No I/O.
│  ├─ vxl-net/           # iroh Endpoint, gossip per Sector, membership/ACL, ping/loss stats, media stream mux
│  ├─ vxl-store/         # SQLite: signed message log, sectors, settings
│  ├─ vxl-audio/         # cpal I/O, RNNoise, Opus, jitter buffer, mixer, Zero-Floor Gate, PTT
│  ├─ vxl-aux/           # slash commands, queue CRDT, DJ election, yt-dlp manager, WebM/Ogg demux, local-file fallback
│  ├─ vxl-telemetry/     # WGC capture → Media Foundation HW H.264 → packetizer
│  ├─ vxl-metrics/       # lock-free atomic counters + 4 Hz snapshot stream
│  ├─ vxl-pack/          # .vxl pack format (zip + manifest.toml + ed25519 signature)
│  └─ vxl-platform/      # Windows impls behind traits: MMCSS, LL keyboard/mouse hook, process memory, tray raster
├─ apps/desktop/
│  ├─ src-tauri/         # thin adapter: commands, events, tray, deep links, Zero-Drag lifecycle
│  └─ src/               # SolidJS UI; src/styles/tokens.css is the design-token source of truth
└─ tools/vxl-cli/        # headless peer used for multi-peer testing on one machine
```

### Crate boundary rules
1. **All logic lives in `crates/`.** `src-tauri` only translates between Tauri commands/events and crate APIs.
2. `vxl-core` has **no I/O and no async runtime**, only types, codecs and pure logic.
3. Dependency direction: `core ← metrics ← {store, net, audio, platform} ← {aux, telemetry} ← apps`. No cycles, and no `apps` → `apps`.
4. Only `vxl-platform` (and `vxl-telemetry` for capture/encode) may `use windows::*`. Everything else goes through traits.
5. The UI never talks to the network or audio directly. It sends commands and subscribes to events/channels.
6. Library crates use `thiserror`. `anyhow` is allowed only in binaries (`src-tauri`, `vxl-cli`).

---

## 4. Real-time & performance rules (non-negotiable)

### Audio callbacks (cpal input/output closures)
- **No allocation, no locks, no syscalls, no logging, no `await`.** Only `rtrb` SPSC ring push/pop and arithmetic.
- DSP/encode runs on a dedicated thread registered with MMCSS `"Pro Audio"` (`AvSetMmThreadCharacteristicsW`).
- Opus frames are **10 ms @ 48 kHz** with DTX on. Voice target ≤ 40 kbps per peer.
- Jitter buffer is adaptive between 20 and 80 ms, with Opus PLC/FEC on loss.
- Per-frame processing (capture → encode → send) must be **< 1 ms p99**. Check it with `tracing` spans.

### Memory
- **Zero-Drag Mode target: < 35 MB working set** (voice only). This is only reachable by **destroying the webview window**, not hiding it. WebView2 alone uses 60–150 MB.
- Global allocator: `mimalloc`. Preallocate buffers on hot paths. Hot paths use no `Vec` growth and no `String` formatting.
- Metrics are `AtomicU64`/`AtomicU32` counters, sampled at 4 Hz and pushed to the UI. Do not poll from the UI.

### UI
- Solid signals per metric, so a ticking value updates only its own text node.
- No `backdrop-filter`, no blur, no stacked box-shadows. Animate only `transform` and `opacity`.
- Pushing metrics to the UI must not happen more often than 4 Hz (voice rings may use 30 Hz through a dedicated channel).

### Input
- Push-to-talk uses a **non-consuming `WH_KEYBOARD_LL` / `WH_MOUSE_LL` hook** on its own thread.
  **Never** use `RegisterHotKey` or tauri-plugin-global-shortcut for PTT, because they swallow the key from the game.

---

## 5. Networking model (summary)

- **Identity:** one ed25519 keypair per install, which is also the iroh NodeId. Profile records are signed.
- **Sector:** 32-byte `SectorId`, which is also the gossip topic. The owner signs membership and role ops in an append-only ACL log.
- **Sector Key:** `vxl://s/<base32(MessagePack{ sector_id, bootstrap NodeAddrs, owner-signed invite cap, expiry })>`.
  Handled by tauri-plugin-deep-link plus single-instance forwarding.
- **Control plane:** gossip carries presence, chat, typing, queue ops and channel state.
- **Text history:** per-author, hash-chained, signed append logs in SQLite. On reconnect, pull missing ranges from any online peer (version-vector diff).
- **Voice:** full-mesh QUIC **datagrams**.
- **Direct Telemetry:** the streamer fans out H.264 over QUIC uni-streams (default viewer cap 5). Viewers decode with **WebCodecs** in WebView2. Rust only forwards NAL units through a binary Tauri channel.
- **Aux:** the DJ Node streams passthrough Opus frames with shared-epoch timestamps. Every listener has a local volume (0–200%) and mute, applied in their own mixer.
- **AUX failover:** eligible peers are sorted deterministically. The DJ heartbeats `{track, epoch_start, position}` every 500 ms. After 3 missed heartbeats the next peer re-resolves the track, seeks, and resumes.
- **yt-dlp:** stored at `%AppData%/vexel/bin/yt-dlp.exe`. Downloaded only from official GitHub releases and **SHA-256 verified** against `SHA2-256SUMS`. A non-blocking `-U` runs on startup. Format: `bestaudio[acodec=opus]`, streamed through `-o -`.

---

## 6. Commands

pnpm is not on PATH on this machine, so prefix pnpm commands with `corepack`, or run `corepack enable pnpm` once from an elevated terminal.
Tauri's before-hooks already use `corepack pnpm`. Run the commands from the repo root unless noted.

```bash
corepack pnpm install                 # UI deps (pnpm workspace: apps/*)
corepack pnpm dev                     # tauri dev: Vite on :1420 + Rust shell
corepack pnpm typecheck               # tsc --noEmit for the UI
corepack pnpm web:build               # Vite build into apps/desktop/dist (REQUIRED before any cargo step on a fresh clone)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p vxl-cli -- info          # headless peer (binary name: vxl)
cargo run -p vxl-cli -- mem           # print core working set
corepack pnpm build                   # tauri build: NSIS + MSI installers
corepack pnpm icons                   # regenerate app icons from assets/brand/vexel-mark.svg
```

Gotchas:
- `tauri::generate_context!` needs `apps/desktop/dist/` to exist at compile time, so build the UI before running clippy or tests on a clean checkout. CI does this.
- The UI renders in a plain browser too (no IPC, so metrics stay at `--`). Keep Tauri API calls lazy (inside handlers or `onMount`) so this keeps working.
- CI: `.github/workflows/ci.yml` runs on windows-latest (typecheck, UI build, fmt, clippy, test, release build).

Release profile: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`, `opt-level = 3`.

---

## 7. Conventions

- Rust edition 2024. `#![deny(unsafe_op_in_unsafe_fn)]`. Every `unsafe` block gets a `// SAFETY:` comment.
- Errors: one `Error` enum per crate (`thiserror`). No `unwrap()`/`expect()` outside tests and startup invariants.
- Logging: `tracing`. Never log on the audio thread. Never log message contents or keys.
- TS: `strict: true`. Components in `PascalCase.tsx`. Every color and spacing value comes from `tokens.css` variables, never hex literals in components.
- Tests: unit tests next to the code. Multi-peer tests drive `vxl-cli` instances.
- Commit only when the user asks. End commit messages with the attribution line the harness supplies.

---

## 8. Known constraints & honest caveats

- "Sub-millisecond" means **internal processing latency**. Mouth-to-ear latency also includes network RTT, the 10 ms frame and the jitter buffer, and the HUD shows those real numbers.
- With $0, the installer is unsigned, so Windows SmartScreen will warn on first install.
- n0 public relays are best-effort with no SLA. A self-hosted `iroh-relay` is the upgrade path.
- yt-dlp playback is against YouTube's ToS. It stays purely user-local, with each DJ Node running its own copy and no central server.

---

## 9. Phase Status

| # | Phase | Status | Done when |
|---|---|---|---|
| 0 | Foundation: workspace, Tauri 2 + Solid shell, tokens.css, fonts, icon, CI, `vxl-cli` stub | ✅ Done locally (CI not yet run: no remote) | Branded window launches; CI green |
| 1 | Identity + Sectors + Link: keypair, Sector Key, deep link, gossip presence, ping/loss HUD | ⏳ Next | Two peers join via Sector Key with live ping |
| 2 | Text comms: signed log, SQLite, gossip chat, offline pull-sync | ⬜ | Offline peer catches up on reconnect |
| 3 | Comms Channels: cpal/Opus/RNNoise/jitter/mixer, Zero-Floor Gate, PTT, MMCSS | ⬜ | 3+ peers talk; 0.0 KB/s when silent |
| 4 | Zero-Drag Mode + dynamic tray icon + RAM telemetry | ⬜ | Minimized voice-only < 35 MB |
| 5 | Aux Channel: slash cmds, queue CRDT, yt-dlp, passthrough, DJ failover | ⬜ | `/play` works; DJ kill resumes mid-track |
| 6 | Direct Telemetry: WGC, MF HW encode, fan-out, WebCodecs viewer | ⬜ | 1080p60 to 3 viewers with encoder badge |
| 7 | .vxl packs + ship: packs, file assoc, updater, installer | ⬜ | Installable auto-updating GitHub release |

Environment notes: Rust 1.99.1 (MSVC) and Node 24 are installed. pnpm 12 runs through corepack (see §6).
Pinned stack at Phase 0: Tauri 2.12 (not the 3.x alpha), SolidJS 1.9, Vite 8, TypeScript 7, windows-sys 0.61.

### What Phase 0 shipped
- Real code: `vxl-core::brand` (brand constants + `hud()` joiner), `vxl-metrics` (lock-free `Counter`/`Gauge`, 4 Hz `Sampler` → `Snapshot`),
  `vxl-platform::memory::working_set_bytes()`. All other crates are documented stubs.
- The desktop shell emits a `telemetry:core` event at 4 Hz. The UI's bottom strip shows the real core working set. Ping, loss and KB/s show `--` or 0 until Phase 1.
- Frameless window with a custom title bar, Sector rail, channel panel, and an empty state whose disabled CTAs wait on Phase 1.
