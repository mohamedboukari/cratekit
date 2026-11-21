# Cratekit: UI

The UI is a real React app in `ui/`. In the desktop app it talks to Rust; in a browser it runs on a typed mock of the same API, so every screen can be opened, clicked and tested without Rust:

- **Live demo:** <https://mohamedboukari.github.io/cratekit/> (mock data, rebuilt on every push to `main`)
- **Locally:** `bun run --cwd ui dev`, then open <http://localhost:5173> (mock mode)
- **Desktop:** `bun run tauri dev` (real Rust; a command not built in Rust yet shows a calm "Not built yet" note, not an error)

Stack: **React 19 + TypeScript 7 + Vite 8**, package manager **bun**, tests with **Vitest**. Components: **Tailwind CSS v4 + shadcn/ui** (style `base-mira`: compact, Base UI primitives, Lucide icons). State: **Zustand**. Track table: **TanStack Table v9 + TanStack Virtual**. The UI renders state and sends commands; all domain logic lives in Rust (see `docs/ARCHITECTURE.md`, section "UI").

## Design tokens

Defined once in `ui/src/index.css` as Tailwind v4 `ck-*` colours (`bg-ck-panel`, `text-ck-muted`, ...), mapped onto shadcn's semantic colours (`primary` = lime, `card` = panel, `border` = line, ...). Components use the semantic or `ck-*` classes, never raw colours.

| Token | Value | Used for |
| --- | --- | --- |
| `ck-win` | `#0f1114` | window background |
| `ck-side` | `#0b0c0f` | sidebar, status bar |
| `ck-panel` / `ck-panel2` | `#15181d` / `#1b1f25` | cards / inputs, chips, inactive buttons |
| `ck-line` | `#23272e` | borders |
| `ck-text` / `ck-muted` | `#e9ebee` / `#858c96` | text / secondary text |
| `ck-lime` | `#c6f432` | primary actions, BPM and Camelot, active tab, focus ring |
| `ck-ink` | `#0c0e11` | text on lime and on cue colours |
| `ck-orange`, `ck-blue`, `ck-pink`, `ck-purple`, `ck-lime`, `ck-yellow`, `ck-red`, `ck-teal` | | the 8 hot cue colours (`components/deck/cueColors.ts`); stems use pink (vocals), orange (drums), blue (bass), teal (other) |

- Fonts: **Space Grotesk** (UI text) and **JetBrains Mono** (numbers: BPM, time, LUFS, MIDI), bundled through `@fontsource-variable/*` (offline app). License: SIL OFL-1.1, approved for fonts. Numbers use `font-mono tabular-nums`.
- Base size 13 px. Radius 6 to 8 px for controls, 10 to 12 px for cards. Borders 1 px.
- Dark theme only for v0.1.
- Canvas code (`ui/src/canvas/`) reads the same tokens with `getComputedStyle` once per resize, so colours live in one place.

## Layout

Laptop-first: designed for 1366x768, works down to the 1024x640 minimum window. Every pane is resizable (`react-resizable-panels`), sizes persist in settings, and the minimum widths add up to fit 1024 px.

```
┌──────────────────────────────── native titlebar and menu (Settings... Cmd/Ctrl+,) ───────────────────┐
│ Sidebar        │ Deck: header and stats · waveform with cue flags · transport · filter knob beside    │
│ library counts │──────────────────────────────────────────────────────────────────────────────────────│
│ crates         │ Track browser                        │ Set list         │ Tools: Analyze · Master ·  │
│ devices        │ search · filter chips · virtual table │ (collapsible)    │ Stems · Controller          │
│ Settings       │                                      │                  │ (collapsible)               │
└──────────────── Status bar: controller · job queue · backend · offline · version ─────────────────────┘
```

| Region | Component (in `ui/src/components/`) | Default / min width |
| --- | --- | --- |
| Shell | `layout/AppShell.tsx` | window min 1024x640 |
| Sidebar | `layout/Sidebar.tsx` | 210 / 170 px, collapsible |
| Deck | `deck/Deck.tsx`, `Waveform.tsx`, `Transport.tsx`, `CueEditDialog.tsx` | 224 / 220 px tall |
| Track browser | `browser/TrackBrowser.tsx`, `columns.ts`, `TrackContextMenu.tsx` | min 360 px |
| Set list | `browser/SetList.tsx` (inside `browser/BrowserArea.tsx`) | 260 / 190 px, collapsible |
| Tools | `tools/ToolPanel.tsx` and one panel per tab | 340 / 280 px, collapsible |
| Status bar | `layout/StatusBar.tsx` | 28 px tall |
| Settings | `settings/SettingsView.tsx` in a native window (`SettingsApp.tsx`), or in the page in the browser demo (`SettingsDialog.tsx`) | 760x560 |
| Dialogs | `dialogs/ExportDialog.tsx`, `ModelDownloadDialog.tsx`, `DropOverlay.tsx` | |

## Screens and states

| Screen | What it shows | States |
| --- | --- | --- |
| Deck | Title, artist, BPM (with confidence), key, Camelot, energy, LUFS; canvas waveform (played/unplayed, phrase grid, overview strip, cue ticks); 8 hot cue flags; CUE, play, pads 1 to 4 / 5 to 8, time, "Hearing"; the bipolar filter knob (80 px, LP and HP ends, value always shown) in its own column beside the waveform, where v0.2's channel strip goes | no track loaded; not analyzed (no waveform) |
| Track browser | Crate name, count and total time; search; chips "Matches 8A", "Near 124 BPM", "Has stems" (Rust filters); columns #, title, artist, BPM, key, energy, time, ready tags (energy and tags hide below 36rem) | empty library (drop zone, Add tracks, Add folder); no matches; a running job shows on its rows |
| Set list | Running order with key-clash and BPM-jump warnings from Rust; the track on the deck highlighted (plays through to the next); save as crate; clear | empty (drop target) |
| Analyze | Camelot and key, chroma bars (the key's notes from Rust), mixes-well-with chips that filter the browser, BPM, first downbeat, energy, drop and mix-out (click to seek), loudness, re-analyze | no track; not analyzed; analyzing (progress) |
| Master | Presets (from Rust), intensity, loudness target, reference track (matched in Rust in the B side and the export; a note when it is the deck's own track), level-matched A/B (the B side mastered live), peak and RMS meters, export 24/16-bit | no track; presets loading or failed (Retry); exporting (progress, percentage and Cancel in place of Export; Cancelling... after Cancel; names the track when another is on the deck) |
| Stems | Split time and backend; 4 or 2 stems; per stem: colour, name, waveform, mute, solo, volume; export | no track; model missing (download dialog); not split; splitting (progress, cancel) |
| Controller | Device drawing that lights up from control events; mapping table with Learn | no controller (keyboard shortcut list); listening ("Move a control..."); learned; moved from another control |
| Settings | General (language), Audio output (device, buffer size, sample rate), Library (watched folders, auto-add, output folder), Processing (GPU or CPU), Models (download, delete), Controller (devices), Keyboard (shortcuts) | loading |
| Status bar | Controller, job queue popover (pause, resume, cancel), backend, "Offline · nothing uploaded", version | no jobs |
| Dialogs | VirtualDJ export dry run (changes and backup path, then confirm); model download (size, progress, checksum, cancel); hot cue edit (name, colour, delete); file drop overlay | |

## Interaction rules

- **What you hear follows the tab** (`hooks/useHearing.ts`): Analyze and Controller play the original, Master plays the A/B side, Stems plays the stem mix.
- Waveform: click to seek; drag a cue flag to move it; Delete clears it; right-click, Shift+Enter or F2 on a flag to rename, recolour or delete.
- Pads: click jumps to the cue, or sets it when empty; Shift-click clears.
- Knobs (`controls/Knob.tsx`): click the ring to jump there (not the gap at the bottom); drag from anywhere on the control, up or right to raise, down or left to lower; scroll with a trackpad or wheel (the fingers' direction, also with macOS natural scrolling; a pinch does nothing), Shift for fine moves; the filter clicks into Off near the centre; double-click, Cmd+click (Ctrl+click on Windows and Linux) or Delete resets; arrow keys, Page Up/Down, Home and End when focused. A reset or a key press ends a drag or scroll gesture, so trackpad momentum cannot undo it.
- Track browser: arrows move, Shift extends the selection, Enter or double-click loads, Cmd/Ctrl+A selects all; sort headers are buttons; right-click (or Shift+F10) for the context menu (load, set list, analyze, split stems, mastering reference, VirtualDJ export, crates).
- Set list: drag tracks in, drag or Alt+Arrow to reorder, Delete to remove. It plays through: when a set list track ends on the deck, the next one loads and plays (no blend; blends come with v0.2's second deck), and the track on the deck is highlighted in the list. In-app drags (rows to the set list, reordering) run on pointer events (`lib/drag.ts`: a press becomes a drag after 4 px, Escape cancels, drop slots carry `data-drop-index`, a `data-drop-scroll` area scrolls when the pointer nears its top or bottom, `DragGhost` shows what is dragged), not HTML5 drag and drop, which Tauri's native file drop takes over on macOS and Windows.
- Keyboard (one map in `lib/shortcuts.ts`): Space play/pause, C cue, 1 to 8 hot cues, Q/W/E/R stem mutes, B A/B (Master tab), Cmd/Ctrl+, Settings. Shortcuts never fire while typing. After a mouse click, transport buttons give focus back so Space keeps meaning play/pause.
- Long jobs never block the UI: progress shows in the status bar, on the track rows and in the tab that started them.
- Every control is reachable by keyboard, with a visible focus ring. Dialogs trap focus and close on Escape.
- Drop files or folders anywhere on the window to add them, on every platform (Tauri's native file drop, `onFilesDropped`).
- While a dialog is open, deck shortcuts do nothing (Cmd/Ctrl+, still opens Settings).

## Languages

The UI is translated with **i18next + react-i18next** (#80). **English is the default for every user**, whatever the system language; another language is chosen in **Settings, General, Language** and applies to both windows at once (through `settings_changed`), the page's `lang` attribute and the native menu (`app_set_menu_labels`).

- **Files:** `ui/src/i18n/en/` is the source, one file per UI area (`common`, `deck`, `layout`, `browser`, `dialogs`, `tools`, `settings`, `shortcuts`, `errors`); `ui/src/i18n/fr/` mirrors it. French is typed against English, so a missing or extra key fails `tsc`; a test also checks every value is real text with no em or en dash.
- **Keys:** nested by area and sub-area, short camelCase (`deck.transport.play`, `browser.setList.saveAsCrate`). Plurals use `_one` / `_other` and `t(key, { count })`; values use `{{name}}` interpolation.
- **In code:** components call `const { t } = useTranslation()`; code outside React (stores, `lib/`) calls `i18n.t` from `@/i18n`. Label maps store keys and translate when rendering, so a language switch updates them.
- **Errors:** the toast title is the translated context; the detail is translated by `AppError.code` when known, else Rust's message. Rust stays language-neutral. A command Rust does not have yet becomes the `not_built` code in `ipc.tauri.ts` and shows an info toast ("Not built yet") instead of an error.
- **Background jobs:** named by `jobLabel()` from the job's `kind`, `stage` and track count, never from Rust's English `label`. Lists in sentences (the supported formats) use `Intl.ListFormat` in the current language.
- **Not translated:** music data (titles, artists, crates), key names and Camelot codes, and units (BPM, LUFS, dBTP, dB, CUE). **DJ numbers keep the dot format in every language** (124.0 BPM, -8.0 LUFS, m:ss), like DJ software and hardware; counts, plurals and durations in sentences follow the language.
- **Adding a string:** add it to the English area file, then the same key in every other language (tsc tells you which). Adding a language: see #82.
- French text is drafted for review: a native speaker checks it before each release.

## React rules

- **IPC only through `ui/src/lib/ipc.ts`.** Components, hooks and stores call `api()`; only `ui/src/lib/ipc.tauri.ts` calls `invoke()`, `listen()` or Tauri plugins. `ui/src/mock/api.ts` implements the same contract with fixtures; it is loaded only when `import.meta.env.DEV` or `VITE_MOCK=1`, so production builds never contain it.
- **No business logic in the UI.** No BPM maths, key matching, Camelot tables, thresholds, loudness maths or file paths: Rust sends the answer (for example `keyCompatible`, `bpmJump`, `chromaInKey`, preset targets, export paths). The mock stands in for Rust and is the only place such rules are faked.
- **30 Hz data stays out of React state.** `playback_position` and `meter` write into refs (`hooks/usePlaybackRefs.ts`); canvases redraw in a `requestAnimationFrame` loop. Controller activity sets DOM attributes directly.
- **State:** one Zustand store per domain in `ui/src/stores/` (library, player, jobs, settings, ui, devices, setlist, master, stems, controller). Async actions use sequence guards so a slow answer never overwrites a newer one.
- **Subscriptions live in hooks** (`ui/src/hooks/`) with cleanup on unmount. `useIpcEvents` wires Rust events into the stores once, in the app root.
- One function component per file, PascalCase `.tsx`. TypeScript strict, no `any`, no unsafe `as`, explicit return types.
- Tests: Vitest with Testing Library in `ui/src/**/*.test.ts(x)`, against the mock API.
