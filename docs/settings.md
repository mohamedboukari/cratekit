# Cratekit: settings

`crates/cratekit-core/src/settings.rs` defines the app settings (SPEC, Module 7); `src-tauri/src/settings.rs` exposes them. Tested with `cargo nextest run -p cratekit-core settings`.

## `Settings`

The UI's `Settings` (`ui/src/lib/types.ts`), with the defaults of `ui/src/lib/defaults.ts`. Rust owns the defaults and the rules.

| Key (as the UI sends it) | Type | Default |
| --- | --- | --- |
| `language` | `"en"` or `"fr"` | `"en"` for every user, never taken from the system |
| `audioDeviceId` | string or null | null (the system default output) |
| `bufferSize` | 128, 256, 512 or 1024 (`BUFFER_SIZES`) | 512 |
| `watchedFolders` | list of paths | empty |
| `watchFolders` | boolean | false |
| `outputFolder` | path | empty (the app's default folder) |
| `stemsBackend` | `"auto"` or `"cpu"` | `"auto"` |
| `paneLayouts` | panes by group, sizes in percent | empty |
| `setList` | track ids, in order | empty |

## Rules

- `Settings::with(key, value)` changes one key and checks it: an unknown key, a value of the wrong type, or a rule the type cannot express (the buffer size list) is `CoreError::InvalidSetting` (code `invalid_setting`).
- **Storage:** the library's `settings` table holds one JSON value per key (`Library::set_setting` stores one row; `Library::settings` reads them over the defaults). A stored value that no longer loads (written by another version, or damaged) keeps its default, and the rest still load.
- **In the app:** `settings_get` returns every setting; `settings_set(key, value)` stores one and sends `settings_changed` with all of them to every window, so the main window and the Settings window agree.

## Not yet

Applying the buffer size and the chosen device to the audio output (#20), and watching folders for new files (SPEC Module 1).

## Tests

The defaults match the UI's; values of the right type are set (including null for the device); unknown keys, wrong types and a bad buffer size are refused; stored values load over the defaults and bad ones are skipped; a stored value round-trips; settings survive reopening the library and a refused value stores nothing.
