# BRAND_SYSTEM.md — VEXEL "F1 Carbon Telemetry"

> **VEXEL // SUB-MILLISECOND COMMS** — *"Sub-Millisecond Comms. Zero Engine Drag."*

**Ideology:** Formula 1 telemetry dashboards meet esports HUDs. The UI is stripped to raw carbon fiber, and every
number it shows is real and live. If something costs RAM, bandwidth or latency, the UI makes that cost visible.
Nothing is decorative unless it carries data.

`apps/desktop/src/styles/tokens.css` must mirror this document exactly. Components only reference tokens and never hard-code hex values.

---

## 1. Color palette

| Token | Hex | Name | Use |
|---|---|---|---|
| `--vxl-bg` | `#0B0C0E` | Carbon Matte | App background. True ultra-dark |
| `--vxl-surface` | `#16181D` | Tactical Titanium | Panels, sidebars, cards, inputs |
| `--vxl-border` | `#1E2028` | Titanium Seam | All borders: `1px solid` |
| `--vxl-border-icon` | `#1B1D24` | Icon Seam | App-icon inner stroke only |
| `--vxl-volt` | `#D4FF00` | **Telemetry Volt** (hero) | Active voice ring, VAD gate indicator, `0% PKT LOSS`, primary CTAs, focus ring |
| `--vxl-amber` | `#FF9900` | Aux Amber | Aux Channel waveform, queue progress, `AUX // BOT`, moderate ping `30–80 ms` |
| `--vxl-red` | `#FF334B` | Laser Red | `ON AIR` badge, muted-mic slash, high loss / ping > 80 ms, destructive actions |
| `--vxl-text` | `#F4F5F7` | Primary text | Body, names, headings |
| `--vxl-text-muted` | `#8A909E` | Muted text | Labels, timestamps, secondary metadata |

### Derived tokens (the only allowed derivations)
| Token | Value | Use |
|---|---|---|
| `--vxl-surface-hover` | `#1B1E24` | Hover on surface rows |
| `--vxl-surface-active` | `#20232B` | Selected row / pressed |
| `--vxl-volt-dim` | `rgba(212,255,0,0.12)` | Volt fills behind badges, selected channel tint |
| `--vxl-amber-dim` | `rgba(255,153,0,0.12)` | Amber badge fill |
| `--vxl-red-dim` | `rgba(255,51,75,0.12)` | Red badge fill |
| `--vxl-on-volt` | `#0B0C0E` | Text/icons placed on a volt fill |

### Semantic status mapping
| State | Color |
|---|---|
| Ping `< 30 ms`, loss `0%`, speaking, gate open | Volt |
| Ping `30–80 ms`, loss `0.1–2%`, Aux activity | Amber |
| Ping `> 80 ms`, loss `> 2%`, muted, ON AIR, error | Red |
| Idle / gated / offline | Muted text |

### Hard rules
- **No glassmorphism, no `backdrop-filter`, no blur, no gradients on surfaces, no drop-shadow stacks.** Depth comes only from `bg → surface` contrast and 1px seams.
- Volt is precious. Use **at most one volt CTA per view**. Volt means "live / go / healthy".
- Never put volt text on a white or light background. On a volt fill, use `--vxl-on-volt`.

---

## 2. Typography

| Role | Family | Token | Notes |
|---|---|---|---|
| UI & chat | **Geist Sans** (fallback: Inter, system-ui) | `--vxl-font-ui` | Self-hosted woff2, no CDN |
| Telemetry, ping, bitrate, RAM, keys, badges | **JetBrains Mono** | `--vxl-font-mono` | Always `font-variant-numeric: tabular-nums;` |

### Scale (px / line-height)
| Token | Size | LH | Weight | Use |
|---|---|---|---|---|
| `--vxl-fs-xs` | 11 | 14 | 500 mono, uppercase, `letter-spacing: 0.08em` | Badges, HUD labels |
| `--vxl-fs-sm` | 12 | 16 | 400 | Timestamps, meta |
| `--vxl-fs-md` | 14 | 20 | 400 | Chat body, lists (default) |
| `--vxl-fs-lg` | 16 | 22 | 600 | Panel titles, Sector name |
| `--vxl-fs-xl` | 20 | 26 | 600 | Section headers |
| `--vxl-fs-hero` | 32 | 36 | 700 mono | Splash / big live metric |

- Labels in HUDs are **UPPERCASE mono** joined with ` // ` (space, double slash, space).
- Metrics keep a fixed width (tabular numerals, zero-padded where sensible) so values never jitter as they tick.

---

## 3. Spacing, radius, motion

- Spacing grid is a **4 px base**: `--vxl-space-1..8` = `4, 8, 12, 16, 20, 24, 32, 48`.
- Radius: `--vxl-radius-sm: 4px` (badges, inputs), `--vxl-radius-md: 8px` (panels, cards), `--vxl-radius-lg: 12px` (modals). Avatars use `50%`.
- Borders: always `1px solid var(--vxl-border)`. A focus ring is `1px solid var(--vxl-volt)` plus `outline-offset: 1px`.
- Motion: `--vxl-ease: cubic-bezier(0.2, 0, 0, 1)`, durations `80ms` (hover), `140ms` (state), `200ms` (panel). Animate **only `transform` and `opacity`**. Honor `prefers-reduced-motion`.
- Voice ring: a 2px volt ring whose opacity follows voice level at up to 30 Hz. It must not use scale pulsing or glow.

---

## 4. HUD / badge formats (exact strings)

Everything is mono, uppercase and tabular. `//` separates segments. `|` separates metrics inside one segment.

| Element | Format example | Color |
|---|---|---|
| OS tag | `VEXEL // SUB-MILLISECOND COMMS` | Muted text, volt `VEXEL` |
| Live metric strip | `12ms \| 0.0 KB/s \| 31.4 MB` | By status mapping |
| Packet loss | `0% PKT LOSS` / `1.2% PKT LOSS` | Volt / Amber / Red |
| Zero-Floor Gate | `ZERO-FLOOR // 0.0 KB/s` | Volt when gated-silent |
| Direct Telemetry encoder | `NVENC // 1080p60`, `AMF // 1440p60`, `QSV // 720p30` | Muted text on surface |
| On air | `ON AIR // 60 FPS` | Laser Red fill, white text |
| Zero-Drag tray | `ZERO-DRAG ARMED // 31.4 MB` | Tray tooltip |
| Aux participant | `AUX // BOT` | Aux Amber |
| DJ host | `DJ NODE // <name>` | Aux Amber |
| Sector Key | `vxl://s/…` (truncated middle, copy button) | Mono, muted |

Units: `ms`, `KB/s`, `MB`, `FPS`, `kbps` (for codec settings only). One decimal for KB/s and MB. Integer ms.

---

## 5. Components (canonical look)

- **Sector rail (far left):** 48 px squircle icons on `--vxl-bg`. The active Sector gets a 3 px volt bar on its left edge.
- **Channel list:** surface panel with rows 32 px high. A Comms Channel row shows connected members with voice rings. Members who are speaking get a volt ring, and muted members show a red mic slash.
- **Chat:** messages sit on `--vxl-bg` with no bubbles. The name is weight 600 and the timestamp is muted mono xs.
- **Telemetry strip (bottom bar):** always visible, mono xs. Shows ping, loss, up/down KB/s, RAM and the Zero-Floor state.
- **Buttons:** Primary has a volt fill with `--vxl-on-volt` text. Secondary is a surface with a 1px seam. Destructive is a red outline. Height 32 px, radius sm.
- **Aux deck:** surface card with an amber waveform/progress bar, the queue list, a `DJ NODE // <name>` header, and a per-listener volume slider (0–200%) plus mute.
- **Direct Telemetry tile:** canvas with an `ON AIR // 60 FPS` badge top-left and the encoder badge top-right.

---

## 6. Logo / app icon / tray mark

Canonical geometry (512×512). Use it unchanged for the window header, splash, installer and app icon.
*(The pasted spec had a markdown-mangled `xmlns`. This is the corrected value.)*

```xml
<svg width="512" height="512" viewBox="0 0 512 512" fill="none" xmlns="http://www.w3.org/2000/svg">
  <rect width="512" height="512" rx="96" fill="#0B0C0E"/>
  <rect x="6" y="6" width="500" height="500" rx="90" stroke="#1B1D24" stroke-width="4"/>
  <path d="M96 128H176L256 324L216 400L96 128Z" fill="#F4F5F7"/>
  <path d="M416 128H336L240 362L260 400L416 128Z" fill="#D4FF00"/>
  <polygon points="236,128 276,128 256,172" fill="#D4FF00"/>
</svg>
```

### Dynamic tray states
The two `#D4FF00` elements (right chevron and apex notch) are the **live state channel**. They are rasterized at runtime with resvg:

| State | Live-element fill |
|---|---|
| Connected, idle | `#D4FF00` at 45% opacity |
| You are speaking | `#D4FF00` solid |
| Ping 30–80 ms | `#FF9900` |
| Ping > 80 ms / disconnected / muted | `#FF334B` |
| Offline (no Sector) | `#8A909E` |

The white left stroke (`#F4F5F7`) and the carbon plate never change. Render the tray icon at 16, 20, 24 and 32 px. At ≤ 20 px, drop the inner border rect.

### Usage rules
- Minimum size 16 px. Clear space is at least 1/8 of the icon width on all sides.
- Never recolor the white stroke, never add glow or shadow, never place the icon on a light background without its carbon plate.
- Wordmark: `VEXEL` in JetBrains Mono 700, `letter-spacing: 0.12em`, `--vxl-text`. Optionally follow it with a muted `// SUB-MILLISECOND COMMS`.

---

## 7. Voice & copy

- Terse, technical and present tense, like a pit-wall radio: "Sector Key copied.", "DJ Node handed off to Mika.", "Zero-Drag armed."
- Use the brand vocabulary from `CLAUDE.md §2`. Never say "server", "voice channel", "go live" or "music bot".
- Errors state the cause and the number: `LINK LOST // RELAY TIMEOUT 3000ms`, not "Something went wrong."
- No emoji in system UI.

---

## 8. Do / Don't

| Do | Don't |
|---|---|
| Show real live numbers in mono | Show fake or placeholder metrics |
| Use 1px seams for structure | Use shadows, blur or glass for depth |
| Keep volt for "live / healthy / primary" | Spray volt on decoration |
| Use tabular numerals for anything that ticks | Let widths jitter as numbers change |
| Animate opacity and transform | Animate width, height, box-shadow or filter |
