# Cratekit: analysis module

`crates/cratekit-core/src/analysis/`: track analysis (SPEC, Module 2). Pure Rust, no Tauri; tested with `cargo nextest run -p cratekit-core`. Detectors arrive task by task: key and Camelot now (#11); tempo and key detection (#10), loudness and energy (#12), waveform columns (#13) and `analyze_track()` later.

## Layout

| File | Contents |
| --- | --- |
| `mod.rs` | Module root; re-exports the submodules |
| `key.rs` | Musical keys and the Camelot wheel (#11). The `KeyDetector` trait joins with #10 |

## `key.rs`: keys and the Camelot wheel

**Public types**

| Type | What it is |
| --- | --- |
| `PitchClass` | The 12 notes, C to B (sharps). `index()` 0 to 11, `from_index(i)` wraps every octave, `name()` such as `"C#"`, `ALL` in order |
| `Mode` | `Major` or `Minor` (natural minor scale) |
| `Key { tonic, mode }` | A key. Displays as `"A minor"`, `"C# major"` (the UI's `keyName`) |
| `Letter` | `A` (minor) or `B` (major); `other()` flips it |
| `Camelot` | A wheel position. Private fields: every way to make one (`new`, `Key::camelot`, parsing) checks the number is 1 to 12. Displays and parses as `"8A"` (the UI's `camelot`) |
| `ParseCamelotError` | Why a string is not a code: `Empty`, `Letter`, `Number` |

**Main functions**

| Function | Rule |
| --- | --- |
| `Key::camelot()` | Each fifth (7 semitones) is one step round the wheel; C major is 8B. A minor key takes its relative major's number (3 semitones up): A minor and C major are both 8 |
| `Camelot::key()` | The exact reverse of `Key::camelot()` |
| `Camelot::matches(other)` | Mixes well: the same code, one step apart with the same letter (12 and 1 are neighbours), or the same number with the other letter |
| `Camelot::mixes_well_with()` | The three partners, in the UI's order: relative key, one step down, one step up (the code itself is left out). The UI's `mixesWellWith` |
| `Key::scale()` | Which of the 12 pitch classes (C to B) are in the key: major or natural minor, always 7. The UI's `chromaInKey` |
| `"8A".parse::<Camelot>()` | A number 1 to 12 then `A` or `B`, either case, spaces around ignored; anything else is a typed error |

**Tests** (in `key.rs`): all 24 keys against the SPEC table typed out by hand, the round trip key to code to key, parsing of every code, exactly four matches per code (itself plus three partners) and symmetry, the 12 to 1 wrap, parse errors, scale contents (C major is the white keys, relative keys share a scale), names. The module doc example runs as a doc test.
