# Cratekit: UI

The visual reference is **`docs/ui-preview.html`** (open it in a browser; it has an animated tour). Match it closely. Everything in it is static mock data: replace with real state from Rust.

Stack: **React 19 + TypeScript 7 + Vite**, package manager **bun**, tests with **Vitest**. Components: **Tailwind CSS v4 + shadcn/ui** (style `base-mira`: compact, Base UI primitives, Lucide icons). The UI renders state and sends commands; all domain logic lives in Rust (see `docs/ARCHITECTURE.md`, section "UI").

## Design tokens

Defined once in `ui/src/index.css` as Tailwind v4 `ck-*` colours (e.g. `bg-ck-panel`, `text-ck-muted`), and mapped onto shadcn's semantic colours (`primary` = lime, `card` = panel, `border` = line, ...):

```css
--bg:      #08090b;   /* outside the window (web preview only) */
--win:     #0f1114;   /* window background */
--side:    #0b0c0f;   /* sidebar */
--panel:   #15181d;   /* cards */
--panel2:  #1b1f25;   /* inputs, chips, inactive buttons */
--line:    #23272e;   /* borders */
--text:    #e9ebee;
--muted:   #858c96;
--accent:  #c6f432;   /* lime: primary actions, BPM/Camelot, active tab */
--orange:  #ff7a45;   /* hot cue 1, muted stem */
--blue:    #4cc9f0;   /* hot cue 2, bass stem, energy bars */
--pink:    #f72585;   /* hot cue 3, vocals stem */
--purple:  #9b5de5;   /* hot cue 4, other stem */
```

- Fonts: **Space Grotesk** (UI text), **JetBrains Mono** (numbers: BPM, time, LUFS, MIDI values), bundled locally through the `@fontsource-variable/*` packages (offline app; no Google Fonts at runtime). License: SIL OFL-1.1, approved for fonts.
- Base size 13 px. Radius: 6 to 8 px for controls, 10 to 12 px for cards. Borders 1 px `--line`.
- Dark theme only for v0.1.
- Canvas drawing code reads the same tokens (via `getComputedStyle`), so colours stay in one place.

## Layout

```
┌──────────────────────────────── titlebar (menu · title · search) ─────────────────────────────┐
│ sidebar 210px │ DECK: art · title · stats (BPM, Key, Camelot, Energy, LUFS)                    │
│  Library      │       waveform with hot cue markers + playhead · overview bar                  │
│  Crates       │       transport: CUE · Play · pads 1 to 4 · time · "Hearing: ..."              │
│  Tools        ├────────────────────────────────────────────────────────────────────────────────┤
│  Devices      │ TOOL TABS: Analyze · Master · Stems · Controller (two cards side by side)      │
│               ├────────────────────────────────────────────────────────────────────────────────┤
│               │ BROWSER: crate name · filters (Matches 8A, BPM range, Has stems) · track table │
└──────────────── status bar: controller · background jobs progress · GPU · offline · version ───┘
```

## Components

One React function component per file, PascalCase `.tsx`, in `ui/src/components/`. Imperative canvas drawing lives in `ui/src/canvas/` and is wrapped by a thin component.

| Component | Notes |
| --- | --- |
| `Sidebar` | Library counts, crates with colour swatch, tools, connected device card (green dot when connected) |
| `Deck` | Stats boxes (BPM and Camelot in accent colour), `Waveform`, 4 hot cue markers with numbered flags, playhead, overview bar |
| `Waveform` | `<canvas>`: played part in accent gradient, rest grey. Playhead and position come from a ref updated by the `playback_position` event and drawn in a `requestAnimationFrame` loop, never through React state |
| `Transport` | CUE, Play/Pause (accent), hot cue pads in cue colours, time `m:ss / m:ss`, "Hearing: Original / Mastered · Club / Stems mix" |
| `AnalyzePanel` | Big Camelot code, key name, 12 chroma bars (scale-degree bars in accent), "Mixes well with" chips, drop / mix-out times |
| `MasterPanel` | Preset buttons, intensity slider, loudness target, A/B toggle, two loudness meters (`Meter`, canvas), Export WAV |
| `StemsPanel` | 4 rows: colour dot, name, waveform, M/S buttons, volume slider; header shows split time and "cached" |
| `ControllerPanel` | Drawing of the connected controller (jog, 8 pads: 4 hot cues + 4 stem pads, filter and tempo), MIDI mapping table with Learn buttons |
| `TrackBrowser` | Columns: #, Title, Artist, BPM, Key (Camelot, accent when it matches the playing track), Energy (10 small bars), Time, Ready tags (analyzed / stems / mastered). Playing row has a left accent bar. Must stay smooth with 10,000+ tracks (row virtualization, library chosen in Phase 1 after a license check) |
| `StatusBar` | Device, job progress bar with count, GPU on/off, "Offline · nothing uploaded", version |

## React rules

- **IPC only through `ui/src/lib/ipc.ts`.** Components never call `invoke()` or `listen()` directly.
- **30 Hz data stays out of React state.** `playback_position` and `meter` update refs; canvas components redraw on `requestAnimationFrame`. This keeps the deck smooth while the rest of the UI re-renders only on real changes.
- **Subscriptions live in hooks** (`ui/src/hooks/`) with cleanup on unmount.
- No business logic in components: no BPM maths, no key matching, no Camelot tables. Ask Rust (e.g. `library_list` with a key-match filter).
- TypeScript strict, no `any`, no unsafe `as` casts.
- The state library (if any beyond React state and context) and the table virtualization library are still open Phase 1 decisions (new dependencies: check licenses and ask first).

## Interaction rules

- Waveform: click to seek; hot cue flags draggable; right-click a flag to rename / recolour / delete.
- Pads: click = jump to cue (set it if empty). Shift-click = clear.
- Switching tabs changes what you hear: Analyze/Controller = original, Master = A/B selection, Stems = stem mix.
- Long jobs never block the UI: show progress in the status bar and on the track row.
- Every control reachable by keyboard; focus rings in accent colour.
- Empty states: no tracks → big drop zone "Drop tracks or a folder"; no controller → "Plug in a controller or use keyboard shortcuts".
