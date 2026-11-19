# Cratekit: analysis module

`crates/cratekit-core/src/analysis/`: track analysis (SPEC, Module 2). Pure Rust, no Tauri; tested with `cargo nextest run -p cratekit-core --all-features` (the `stratum` wrappers need the feature). Detectors arrive task by task: key and Camelot (#11), tempo and key detection (#10), loudness and energy (#12), the whole-track analysis `analyze_file` (#15) and the waveform columns (#13).

## Layout

| File | Contents |
| --- | --- |
| `mod.rs` | Module root; re-exports the submodules |
| `key.rs` | Musical keys and the Camelot wheel (#11) |
| `tempo.rs` | `TempoDetector` trait and `OnsetTempo`, our BPM detector (#10) |
| `key_detect.rs` | `KeyDetector` trait and `ProfileKey`, our key detector (#10) |
| `loudness.rs` | `measure_loudness`: integrated LUFS and true peak, through the `ebur128` crate (#12) |
| `energy.rs` | `energy_score`: the 1 to 10 energy score (#12) |
| `track.rs` | `analyze_file` / `analyze_buffer`: a whole track, with progress and cancellation (#15) |
| `waveform.rs` | `Waveform`: min, max and RMS columns of the whole track, stored with the analysis (#13) |
| `stratum.rs` | `StratumTempo` and `StratumKey`: `stratum-dsp` behind the same traits, `stratum` cargo feature (off by default) (#10) |

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

## Detectors (`tempo.rs`, `key_detect.rs`)

Both detectors take mono `f32` samples and a sample rate, and analyze whatever they are given (picking the middle of a track is `analyze_file`'s job). They return `None` when there is nothing to go on: silence, too short, a zero sample rate, or non-finite samples. The traits let implementations be swapped without changing callers: ours are the default, `stratum-dsp` is the second one.

| Item | What it is |
| --- | --- |
| `TempoDetector::detect_tempo(samples, rate)` | `Option<TempoEstimate { bpm, confidence, first_beat_sec }>`: BPM folded into 70 to 180 (`BPM_RANGE`), confidence 0 to 1, and the first beat within one beat of the start of the samples (`None` from `stratum-dsp`) (the UI's `bpm`, `bpmConfidence`, `firstBeatSec`) |
| `OnsetTempo` | Ours. 1: onset envelope, the rise in log energy every 5 ms, smoothed by two 20 ms box averages (a 40 ms triangle, so steady tones and drones do not ripple it). 2: autocorrelation over 55 to 210 BPM, peaks refined with a parabola; the strongest peak is the beat unless a peak at a half, third or quarter of it scores at least 90% (then that is the beat), so off-beat hi-hats never halve or double the tempo. 3: a beat-grid fit every 0.01 BPM within 3%, scored over every onset in the clip, for sub-frame precision. 4: fold into the range. 5: the first beat from the winning grid's phase, minus a measured 6 ms latency of the log-energy rise (`ONSET_LATENCY_SECONDS`). On synthetic kicks at full level the first beat is within 2 ms at every tempo and start time; kicks 12 or 20 dB quieter, or over a sustained pad, land up to about 12 ms late. To re-check on real tracks in #22. No estimate when the beats rise by less than 0.10 in log energy (about a 10% jump, `MIN_ONSET_STRENGTH`); weak beats from 0.10 to 0.15 (`CLEAR_STRENGTH`) also need a confidence of at least 0.75 (`WEAK_BEAT_CONFIDENCE`), and on short clips the floor is at least 0.5 divided by the square root of the beats in the clip (`CHANCE_STRENGTH`: with few beats the best grid can line up with ripple by chance, a 30 Hz drone scores 0.13 over 10 s). Soft real tracks beat at 0.10 to 0.16 with a confidence near 0.8; steady tones stay under 0.10 at 44.1 and 48 kHz (a 36 Hz tone at 48 kHz reaches 0.081, its ripple repeating exactly on a 120 BPM grid of 5 ms frames); crackle, rain and speech-like sounds that reach 0.10 have confidences of 0.42 to 0.70. Confidence: how far the best placement of the winning grid scores above an average placement (independent of clip length) |
| `KeyDetector::detect_key(samples, rate)` | `Option<KeyEstimate { key, confidence, chroma }>`: chroma C to B as loudness relative to the loudest note, 0 to 1 (the UI's `keyName`, `camelot` via `key.camelot()`, `keyConfidence`, `chroma`) |
| `ProfileKey` | Ours. Chroma: Hann-windowed frames of 0.186 s, a Goertzel filter per note from C3 to B6, power summed per pitch class. Key: Pearson correlation with the 24 rotations of the Krumhansl-Kessler (1982) major and minor profiles. Confidence: lead over the runner-up relative to the lead over the average key |

**Speed** (release, 90 s of audio): tempo about 30 ms, key about 330 ms. A whole 3 to 4 minute MP3 (decode, loudness, energy, tempo, key, waveform) takes under 1 s.

**Tests:** slow grooves (75, 85 and 88 BPM, kick, snare and eighth-note hats at two levels) keep their tempo; a soft kick under a louder chord pad is found; sub-bass tones from 30 to 45 Hz give no tempo at 44.1 and 48 kHz; steady tones from 30 to 440 Hz give no tempo; confidence is the same for 4 s and 30 s of the same beat; SPEC acceptance on `synth::test_loop` in C major, A minor, F# major and D# minor at 92, 124, 128 and 140 BPM (BPM within 0.5, key and Camelot correct); kicks alone at the four tempos; folding (60 BPM reads 120, 200 reads 100); every one of the 24 keys from its chords; chroma normalized and strongest in the tonic triad; a single note on its own pitch class; confidences in 0 to 1; no estimate for silence, short, broken or zero-rate input; the same input always gives the same answer.

## Ours versus `stratum-dsp`

`StratumTempo` and `StratumKey` (`stratum.rs`, behind the `stratum` cargo feature) run `stratum-dsp`'s one-pass analysis and keep the tempo or key; it reports no chroma, so `StratumKey` adds ours. Its own key confidence is 0 on every input we tried, so `StratumKey` reports its key clarity instead. Its fallback answers when it has nothing to go on (40 BPM, or C major with zero clarity) become `None`, and the same 3 s minimum length as ours applies. Its key can differ between runs on the same audio (near ties are broken in a random order inside `stratum-dsp`). Measured in release on the same audio:

| 16 SPEC loops, 30 s each (4 keys at 92, 124, 128, 140 BPM) | Ours | `stratum-dsp` 1.0 |
| --- | --- | --- |
| Tempo within 0.5 BPM | 16/16 | 12/16 (128 read 127, 140 read 139.6) |
| Correct key | 16/16 | about 11/16, varies between runs (A minor read A major; D# minor read F# major at 92) |
| Time for all 16 | 1.9 s | 10.4 s |
| 75 BPM groove with eighth-note hats | 75.0 | 149.7 (double) |
| Steady 55 Hz drone | no tempo | 126 BPM |

Our detectors are the default and the app does not ship `stratum-dsp`. Synthetic loops are not real music: re-run this comparison on the test DJ's tracks (#22) before deciding again. Enable it with `--features stratum` (CI and `bun run check` build and test with all features).

## Real tracks (#22)

The Phase 1 gate compares our BPM and key with VirtualDJ on real tracks. Until those references are in, a first check used 14 Creative Commons tracks by Kevin MacLeod (incompetech.com, CC BY 4.0, kept outside the repository), 10 of which carry a BPM tag from the publisher. Each takes under 1 s to analyze in release.

| Track | Tag | Ours | Result |
| --- | --- | --- | --- |
| Disco con Tutti, Electrodoodle, Funkorama, Monkeys Spinning Monkeys, Pamgaea | 115, 120, 101, 144, 94 | the same | exact |
| Carefree | 96 | 96.0 | exact (before: no tempo; its soft beats rose 0.10 with a clear grid, under the old 0.15 floor) |
| Pixelland | 230 | 115.0 | octave: 230 is folded into 70 to 180 by design |
| Cut and Run | 109 | 107.0 | the tag looks wrong: a grid at 107.00 fits the whole 90 s (strength 0.69, confidence 0.92); at 109 it would drift 3 beats over the same span |
| Hitman | 76 | none | orchestral, weak beat (0.14) with an unclear grid (confidence 0.61): no tempo rather than a guess |
| Local Forecast - Elevator | 82 | 109.3 | 4/3 too fast: accents every dotted eighth over a quieter beat repeat more regularly than the beat. A grid at 3/4 of the first guess scores 1.45 times higher here, but a rule built on that flipped correct tempos on synthetic music with loud triplet hi-hats or soft beats under bar-line chord changes (120 read as 90), so it is not used until more real references exist |

6 exact, 1 octave, 1 disputed tag, 2 misses. Key: only Funkorama has a key tag (C); we read D minor (7A). Publisher tags are not ground truth: VirtualDJ's own readings on these and on club tracks are still needed for the gate.
