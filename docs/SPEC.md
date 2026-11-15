# Cratekit: Product spec

## Pitch

A free, open-source desktop app that gives producers and DJs the paid cloud tools around their DAW and DJ software: mastering, stem separation, track analysis and DJ controller support. Everything runs offline on the laptop.

| Cratekit module | Paid tools it replaces |
| --- | --- |
| Analyze | Mixed In Key |
| Master | FL Cloud mastering, LANDR, eMastered |
| Stems | Moises, LALAL.AI |
| Controller + VirtualDJ export | Manual cue prep in DJ software |

## Users

- **DJs** (first user: a VirtualDJ DJ with a Pioneer DDJ-FLX4 controller) who prepare sets: BPM, key, cues, acapellas and instrumentals for edits.
- **Bedroom producers** (FL Studio, Ableton, LMMS) who want a quick master without a subscription.
- **Schools and collectives** that install it on every machine for free.

## Out of scope

- Live mixing / two-deck performance (VirtualDJ, Mixxx do that)
- A DAW, VST hosting, recording
- Streaming or YouTube downloads
- Cloud sync, accounts, telemetry

---

## Module 1: Library

The app keeps a local library of tracks the user adds. Original files are never modified.

**Features**
- Add files or folders (drag and drop, or a file dialog). Supported: MP3, WAV, FLAC, AIFF, M4A/AAC, OGG.
- Watch added folders for new files (optional setting).
- Crates: user-made lists of tracks (a track can be in several crates). Each crate has a name and a colour.
- Track list columns: #, title, artist, BPM, key (Camelot), energy, duration, status tags (analyzed / stems / mastered).
- Search by title, artist, BPM range, key.
- Filters: "matches the playing track's key", BPM range, "has stems".

**Acceptance**
- Adding a folder of 500 tracks shows them in the list within 5 s (tags only); analysis runs in the background.
- Removing a track from the library never deletes the file on disk.

## Module 2: Analyze

**Features**
- BPM (one decimal), with confidence. Normalize to the 70 to 180 range (half/double-time fix).
- Musical key (e.g. "A minor") and Camelot code (e.g. "8A"), with confidence.
- Energy 1 to 10 (derived from loudness, onset density and spectral brightness).
- Integrated loudness (LUFS, ITU-R BS.1770 gating) and true peak (dBTP).
- Waveform data (min/max/RMS per column) for the deck view, cached.
- Beat grid (first downbeat + BPM) and suggested cue points: first beat, drop, breakdown, outro (heuristic; later phase).
- "Mixes well with": same number other letter, ±1 on the Camelot wheel.
- Batch analysis of a whole crate or library, in a background queue with progress, pause and resume.

**Camelot table**

| Pitch class | Major | Minor |
| --- | --- | --- |
| C | 8B | 5A |
| C# / Db | 3B | 12A |
| D | 10B | 7A |
| D# / Eb | 5B | 2A |
| E | 12B | 9A |
| F | 7B | 4A |
| F# / Gb | 2B | 11A |
| G | 9B | 6A |
| G# / Ab | 4B | 1A |
| A | 11B | 8A |
| A# / Bb | 6B | 3A |
| B | 1B | 10A |

**Acceptance**
- Synthetic test loops (kick + bass + chords in a known key) at 92, 124, 128, 140 BPM: BPM within ±0.5, correct key and Camelot code.
- A 6-minute track analyzes in under 5 s on a 2020 laptop CPU.
- Results are cached in SQLite; re-opening the app does not re-analyze.

## Module 3: Master

**Features**
- Presets: **Club, Hip-hop, Pop, Lo-fi, Chill** (parameters in `ARCHITECTURE.md`).
- Intensity slider 0 to 100 % (scales EQ gains, compression ratio and makeup).
- Loudness target in LUFS (default per preset, e.g. Club −8, Chill −12).
- Optional reference track: match the long-term spectrum of a reference, within ±6 dB per band.
- A/B compare while playing, **level-matched** by default (so "louder" does not fool the ear).
- Export WAV 24-bit (and 16-bit with TPDF dither). Output never exceeds −1.0 dBTP.

**Acceptance**
- Exported master measures within ±0.5 LU of the chosen target and ≤ −1.0 dBTP.
- A/B switching is click-free (short crossfade).

## Module 4: Stems

**Features**
- 4 stems: vocals, drums, bass, other (htdemucs). 2-stem mode (vocals / instrumental) = sum of the other three.
- Uses the GPU automatically when one is available, through ONNX Runtime execution providers (CoreML on macOS, DirectML or Windows ML on Windows), and falls back to the CPU when no GPU backend starts or its output is wrong. Settings can force "CPU only". The status bar shows the active backend.
- Model downloaded on first use (~200 MB) with progress and checksum; stored in the app data folder.
- Stems are cached per track; the Stems tab plays them in sync with mute / solo / volume per stem.
- Export: separate WAVs. Later: NI STEM format (for Traktor).

**Acceptance**
- A 4-minute track splits without crashing on a machine with 8 GB RAM.
- Summing the 4 stems reconstructs the original within −30 dB residual.
- The job can be cancelled; the UI stays responsive during the split.
- GPU and CPU backends produce the same stems within the −30 dB residual; a machine with no usable GPU still splits on the CPU without errors.

## Module 5: Controller (MIDI)

**Features**
- Detect USB MIDI devices; hot-plug (connect/disconnect while running).
- MIDI learn: click a target, move a control, mapping saved per device.
- Targets: play/pause, cue, hot cues 1 to 8 (pads show 1 to 4, a page/shift button switches to 5 to 8), stem mute pads (vocals, drums, bass, other), filter knob, tempo fader, volume, A/B toggle (in the Master tab).
- Buttons: Note On (velocity > 0) or CC (value crosses 64). Knobs/faders: CC 0 to 127.
- LED feedback where the controller supports it (pad lights for hot cues set, stems muted).
- Keyboard fallback: Space play, C cue, 1 to 8 hot cues, Q/W/E/R stem mutes, B A/B.
- Ship a default mapping only for controllers we can test (first: Pioneer DDJ-FLX4).

**Acceptance**
- Pad press to audible change in under 20 ms.
- Mappings survive an app restart and a controller unplug/replug.

## Module 6: Export to DJ software

**Features**
- Write BPM, key and hot cues 1 to 8 (position, colour, name) for tracks into **VirtualDJ** (first target).
- Back up the VirtualDJ database file before writing; dry-run preview of the changes.
- Later: Rekordbox XML, Serato tags.

**Acceptance**
- After export, opening VirtualDJ shows Cratekit's hot cues on the right tracks at the right positions.
- **Research first:** confirm the VirtualDJ database format with a real database from our test DJ before coding (see ROADMAP).

## Module 7: Settings

**Features**
- Audio output: device list (hot-plug aware), buffer size, sample rate shown.
- Library: watched folders (add / remove, watch on/off), Cratekit output folder for stems and masters.
- Processing: "GPU (auto)" or "CPU only" for stems, with the detected backend shown.
- Models: download status, size on disk, re-download, delete.
- Controller: connected MIDI devices and their mappings (reset, export, import).
- Keyboard: list of shortcuts.

**Acceptance**
- Switching the audio output device while a track plays resumes playback on the new device without restarting the app.
- Unplugging the active audio device falls back to the system default and shows a notice.
- All settings survive an app restart.

## Non-functional

- Installer under 60 MB per platform (models download separately). Measure it per target in CI.
- Idle RAM under 300 MB; stems job may use up to ~4 GB.
- No network calls except the model download.
- Platforms and CPU architectures, all built and tested in CI:

| OS | Architectures | Package |
| --- | --- | --- |
| macOS (minimum set by Tauri 2 and ONNX Runtime, confirm in Phase 1) | arm64 (Apple Silicon) + x86_64 (Intel), one universal binary | dmg |
| Windows 10/11 | x86_64 + ARM64 | msi |
| Linux (Ubuntu 22.04+) | x86_64 + arm64 | AppImage |

- The GPU is optional: every feature works on CPU only. Only stem separation uses the GPU.
