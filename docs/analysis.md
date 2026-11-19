# Cratekit: analysis module

`crates/cratekit-core/src/analysis/`: track analysis (SPEC, Module 2). Pure Rust, no Tauri; tested with `cargo nextest run -p cratekit-core --all-features` (the `stratum` wrappers need the feature). Detectors arrive task by task: key and Camelot (#11), tempo and key detection (#10), loudness and energy (#12) and the whole-track analysis `analyze_file` (#15) now; waveform columns (#13) later.

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
| `OnsetTempo` | Ours. 1: onset envelope, the rise in log energy every 5 ms, smoothed by two 20 ms box averages (a 40 ms triangle, so steady tones and drones do not ripple it). 2: autocorrelation over 55 to 210 BPM, peaks refined with a parabola; the strongest peak is the beat unless a peak at a half, third or quarter of it scores at least 90% (then that is the beat), so off-beat hi-hats never halve or double the tempo. 3: a beat-grid fit every 0.01 BPM within 3%, scored over every onset in the clip, for sub-frame precision. 4: fold into the range. 5: the first beat from the winning grid's phase, minus a measured 6 ms latency of the log-energy rise (`ONSET_LATENCY_SECONDS`). On synthetic kicks at full level the first beat is within 2 ms at every tempo and start time; kicks 12 or 20 dB quieter, or over a sustained pad, land up to about 12 ms late. To re-check on real tracks in #22. No estimate when the beats rise by less than 0.15 in log energy (about a 15% jump). Confidence: how far the best placement of the winning grid scores above an average placement (independent of clip length) |
| `KeyDetector::detect_key(samples, rate)` | `Option<KeyEstimate { key, confidence, chroma }>`: chroma C to B as loudness relative to the loudest note, 0 to 1 (the UI's `keyName`, `camelot` via `key.camelot()`, `keyConfidence`, `chroma`) |
| `ProfileKey` | Ours. Chroma: Hann-windowed frames of 0.186 s, a Goertzel filter per note from C3 to B6, power summed per pitch class. Key: Pearson correlation with the 24 rotations of the Krumhansl-Kessler (1982) major and minor profiles. Confidence: lead over the runner-up relative to the lead over the average key |

**Speed** (release, 90 s of audio): tempo about 30 ms, key about 330 ms.

**Tests:** slow grooves (75, 85 and 88 BPM, kick, snare and eighth-note hats at two levels) keep their tempo; steady tones from 30 to 440 Hz give no tempo; confidence is the same for 4 s and 30 s of the same beat; SPEC acceptance on `synth::test_loop` in C major, A minor, F# major and D# minor at 92, 124, 128 and 140 BPM (BPM within 0.5, key and Camelot correct); kicks alone at the four tempos; folding (60 BPM reads 120, 200 reads 100); every one of the 24 keys from its chords; chroma normalized and strongest in the tonic triad; a single note on its own pitch class; confidences in 0 to 1; no estimate for silence, short, broken or zero-rate input; the same input always gives the same answer.

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

## Loudness (`loudness.rs`)

`measure_loudness(channels, rate) -> Option<Loudness { integrated_lufs, true_peak_dbtp }>` (the UI's `lufs` and `truePeakDb`). It takes the whole track, one slice per channel (planar), and wraps the `ebur128` crate (MIT, a pure Rust port of the reference libebur128), so it can be swapped.

- **Integrated loudness:** ITU-R BS.1770 / EBU R128. K-weighted power in 400 ms blocks (75% overlap), the -70 LUFS absolute gate, then the relative gate 10 LU below the level of the remaining blocks.
- **True peak:** the highest level between samples, by 4x oversampling below 96 kHz (2x below 192 kHz), maximum over all channels, in dBTP. Never below the highest sample.
- **Channels:** mono, stereo, or 5.1 in the usual order (the LFE is not counted). A mono track counts as one channel: a mono tone reads 3 LU below the same tone on both stereo channels, as BS.1770 says.
- **No answer** (`None`): no channels, channels of different lengths, a sample rate outside 16 Hz to 2.8 MHz, non-finite samples, audio shorter than one 400 ms gating block, or audio entirely below the -70 LUFS gate (silence).

**Tests** (synthetic tones modelled on EBU Tech 3341): a 997 Hz sine at -23 dBFS on both channels reads -23.0 LUFS (within 0.1) at 48 and 44.1 kHz, and -26.0 in mono; added silence and a part 30 dB quieter are gated out; levels move 1 to 1; a quarter-rate sine with every sample at -3 dBFS reads about 0 dBTP; a plain tone's true peak is its peak; the loudest channel sets the true peak; no answer for each unusable input, including audio shorter than 400 ms.

## Energy (`energy.rs`)

`energy_score(samples, rate, integrated_lufs) -> Option<u8>`, 1 (calm) to 10 (peak time), the UI's `energy`. Mono samples, plus the loudness from `measure_loudness`. **A heuristic for ranking tracks, not a measurement:** the ranges and weights below are first guesses checked on synthetic audio, to re-tune on the test DJ's tracks (#22).

| Feature | Weight | 0 | 1 | How |
| --- | --- | --- | --- | --- |
| Loudness | 0.4 | -30 LUFS | -6 LUFS | Integrated loudness, linear between the two |
| Onset density | 0.35 | 0 per second | 8 per second | Peaks of the tempo detector's onset envelope where the log energy rises by at least 0.7 (about +3 dB) within 40 ms. Found twice and merged, at least 50 ms apart: in the signal (kicks, bass notes, chord changes) and in its sample-to-sample difference, which favours highs, so hi-hats over a full mix count too |
| Brightness | 0.25 | 100 Hz | 4 kHz | The RMS frequency (it averages the square of the frequency, weighted by power, so it leans toward the highs), from the RMS of the sample-to-sample difference over the RMS of the signal (for a sine of frequency f the ratio is 2 sin(pi f / rate)); on a log scale; no FFT, a tone reads the same at any sample rate |

Score: `1 + round(9 * weighted sum)`, each feature clamped to 0 to 1. No answer for silence, under 3 s, non-finite samples or loudness, or a zero sample rate.

On the synthetic audio: a quiet 80 Hz tone scores 1, kicks alone 3, the SPEC test loop 6, the loop with off-beat hats 7.

**Speed** (release, 90 s of stereo): loudness and energy together about 0.1 s.

**Tests:** the score stays within 1 to 10; brightness reads the frequency of 100 Hz, 1 kHz and 4 kHz tones within 1% at 44.1, 48 and 96 kHz; one onset per kick at 90, 124 and 174 BPM and at most one (the tone starting) for steady tones from 40 to 440 Hz; 16th-note hats mixed under the full loop count as onsets; a calm tone scores lowest, then kicks alone, then the full loop; hats raise brightness and the score; faster kicks raise the density; a louder copy scores higher with the same brightness; no score for unusable input.

## Whole track (`track.rs`)

`analyze_file(path, progress, cancel) -> Result<TrackAnalysis, CoreError>` decodes the file (`audio::decode_file_with`) and runs `analyze_buffer`:

1. Loudness (`measure_loudness`) on every channel of the whole track.
2. Energy (`energy_score`) on the whole track in mono.
3. Tempo (`OnsetTempo`) and key (`ProfileKey`) on the middle `SECTION_SECONDS` (90 s; the whole track when shorter): intros and outros are often beatless or in another key, and it is faster. The first beat found in the section is moved to track time (the same grid, within one beat of 0).

`TrackAnalysis { duration_sec, sample_rate, channels, tempo, key, loudness, energy }`: a track with no beat, no notes or no sound is not an error, those fields are `None`. Progress goes 0 to 1: decoding is the first 40% (reported every 64 packets when the file announces its length), loudness the next 15% (fed to the meter 10 s at a time, `measure_loudness_with`). `cancel` is checked between steps, every 64 packets while decoding and every 10 s of audio while measuring loudness, and stops the task with `CoreError::Cancelled`. Memory: the decoded track plus a mono copy; the onset envelopes use sliding sums, so they add little (about 5 MB for 10 minutes). `progress::ProgressSink` is any `Fn(f32)` (or `NoProgress`); `progress::CancellationToken` is a shared flag.

**Speed** (release): a 6 minute stereo track (the SPEC loop) in about 1 s, decoding excluded; the SPEC asks for under 5 s on a 2020 laptop CPU. The test enforces the limit on a 6 minute track in release builds (`cargo nextest run --release`); debug builds (CI), many times slower and machine-dependent, analyze a 1 minute track with no time limit.

**Tests:** the SPEC loop gives its tempo, first beat, key, loudness and energy; the first beat of a track whose analyzed section starts 5 s in is still reported near the start; a tone has a key but no beat, silence has nothing; progress runs from 0 to 1 in order; a cancelled token stops the analysis.
