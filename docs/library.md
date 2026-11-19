# Cratekit: library module

`crates/cratekit-core/src/library/`: the user's track library (SPEC, Module 1) in one SQLite file. Pure Rust, no Tauri; tested with `cargo nextest run -p cratekit-core library`. The app side is `src-tauri/src/library.rs`.

## Layout

| File | Contents |
| --- | --- |
| `mod.rs` | `Library`, `prepare_add`, and the types the UI receives: `TrackRow`, `LibraryQuery`, `LibraryCounts`, `Cue`, `CueColor`, `Crate`, `TrackDetail`, `AddResult` |
| `schema.rs` | The schema as numbered migrations (the tables in `docs/ARCHITECTURE.md`, Database) |
| `scan.rs` | Finding audio files in files and folders, `AUDIO_EXTENSIONS`, the file fingerprint |
| `tests.rs` | Tests on the synthetic fixture files |

## Rules

- **Never touches the music.** Adding reads tags and headers; `remove_tracks` deletes rows (and, by cascade, analysis, cues, stems records and crate entries), never files.
- **One writer.** A `Library` owns one connection. The app keeps one behind a lock and calls it on a blocking worker thread. Reads share that connection for now, so they wait while another call holds it; adding is split so the slow part (reading the files) runs without the lock.
- **WAL and foreign keys** on every connection; a 5 s busy timeout for when another connection holds the lock.
- **Schema versions:** 1 is the ARCHITECTURE schema; 2 adds `analysis.chroma` (12 little-endian `f32`).
- **Migrations:** `PRAGMA user_version` counts the steps that ran; opening runs the missing ones, each in its own transaction (a failed step is rolled back). A database with a higher version than this build knows is refused with `DatabaseTooNew`. Never edit a released step; add a new one.

## `Library`

| Method | What it does |
| --- | --- |
| `open(path)` / `open_in_memory()` | Opens or creates the database and migrates it |
| `add_paths(paths)` | `known_paths()`, then `prepare_add(paths, known)`, then `store(prepared)`. Returns `AddResult { added, failed }`: one unreadable file never stops the rest |
| `known_paths()` | The paths of every track |
| `prepare_add(paths, known)` (a function, no `Library`) | Files are taken if they have an audio extension; folders are searched with their subfolders (hidden entries skipped, links to folders not followed). Paths already known, or already queued in this call (a file and its folder, a relative and an absolute spelling), are skipped. New files are probed (`audio::probe_file`) and fingerprinted on up to 8 threads |
| `store(prepared)` | Stores the prepared files in one transaction; a path another add stored in the meantime is skipped (`ON CONFLICT DO NOTHING`) |
| `settings()` / `set_setting(key, value)` | The settings (`cratekit_core::settings::Settings`): stored JSON values over the defaults, a stored value that no longer loads keeping its default; `set_setting` checks the key, the type and the rules before storing one row |
| `transitions(ids)` | How each pair of neighbours in a set list fits: key compatibility (`Camelot::matches`), BPM change in percent, and a jump above `MAX_BPM_CHANGE_PCT` (6%); a track no longer in the library counts as unknown |
| `crates()` | Every crate with its track count, by `sort` (unsorted last), then id |
| `list(query)` | Tracks matching every filter, oldest first. Crate, BPM range, near BPM (within `NEAR_BPM_TOLERANCE` = 3 BPM) and has-stems run in SQL; the search (every word must appear in the title, artist or album, any case, Unicode-aware) and the key match (`Camelot::matches`: the same code, one step either side, or the relative key) run in Rust |
| `counts()` | All tracks, added in the last 7 days, and needing analysis (none stored, or from an older `ANALYZER_VERSION`) |
| `track(id)` | The row, the file path, the stored `Analysis` (as the Analyze tab shows it: key name, Camelot code, scale flags and mixing partners computed here) and the hot cues (slots 1 to 8); `NotFound` for an unknown id |
| `remove_tracks(ids)` | Deletes the rows in one transaction; returns how many existed |
| `store_analysis(id, analysis)` | Stores a `TrackAnalysis` (replacing an older one, with `ANALYZER_VERSION` and the time) and updates the track's length, sample rate and channels from the decoded audio; `NotFound` when the track was removed meanwhile |

- **Titles:** the title tag, or the file name without its extension. The artist is empty without a tag.
- **Paths** are stored absolute (without resolving links). A path that is not valid Unicode is reported in `failed`.
- **Fingerprint:** FNV-1a (64-bit) over the file length and its first and last 64 KiB, as 16 hex digits. Cheap enough for a whole library (128 KiB per file), stable across platforms and Rust versions, and enough to notice a moved or re-encoded file later.
- **Extensions:** `mp3`, `wav`, `flac`, `aiff`, `aif`, `m4a`, `aac`, `ogg`, in any case: the same list as the UI's file picker (`ui/src/lib/formats.ts`).

## In the app

`src-tauri/src/library.rs` opens `cratekit.db` in the app data folder at startup. If that fails, every library command returns the reason (`database`, `database_too_new`, `io`, or `window` when the app data folder cannot be found) instead of the app failing to start. Commands: `library_add_paths` (returns the new ids; holds the library only to read the known paths and to store; unreadable files are skipped, but when nothing was added and some files failed, the first failure is the error so the user sees why), `library_list`, `library_counts`, `track_detail` (with the stored analysis; the waveform is `null` until #13) and `crates_list`. The UI's `TrackRow.activity` stays `null`: running jobs report their tracks through `job_progress` instead. Creating and filling crates come later.

## Tests

- A new library has the latest schema, foreign keys on and no tracks; a file library is in WAL mode and reopens without migrating again; a database from a newer schema is refused.
- Adding a folder: the three audio files in it and its subfolders are added with their tags (or the file name), the hidden file and the text file are skipped, and the fake MP3 is reported as an unsupported format; adding the same files again adds nothing; one file given as an absolute and a relative path is added once; a file another add stored in the meantime is skipped without an error.
- Crates come back in their order with their track counts and colours.
- Settings survive reopening the library; a wrong value is refused and nothing is stored.
- Set list transitions: a relative key fits, a far key does not, 1.6% BPM change is fine, 11.1% is a jump, an unanalyzed or missing track is unknown.
- Search by title, artist and album in any case, every word required; filters by BPM range, near BPM, key match (including an unknown code), stems and crate, on stored analysis rows; counts.
- Track detail with cues in slot order and the default colour; `NotFound` for an unknown id.
- Removing a track keeps its file on disk and removes its analysis.
- A stored analysis comes back with everything the Analyze tab shows, an empty one with empty fields; storing for a removed track is `NotFound`; a version 1 database upgrades and keeps its tracks.
- SPEC acceptance: 500 tracks in 20 folders are added and listed in under 5 s (about 0.1 s in a debug build).
