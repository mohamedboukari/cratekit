# Cratekit: synth module (synthetic test audio)

`crates/cratekit-core/src/synth.rs`: generators for audio with known answers (BPM, key, level), so every DSP function is tested without copyrighted audio (CLAUDE.md, Code conventions; CONTRIBUTING.md, Hard rules). It is compiled only for `cratekit-core`'s own tests, or for other crates' tests through the `test-helpers` cargo feature:

```toml
[dev-dependencies]
cratekit-core = { workspace = true, features = ["test-helpers"] }
```

Release builds never contain it. Output is mono `f32` at the requested sample rate; 0 dBFS is an amplitude of 1.0. Every function is pure (same inputs, same samples).

## Generators

| Function | What it makes |
| --- | --- |
| `sine(freq_hz, level_db, seconds, rate)` | A pure tone peaking at `level_db` dBFS |
| `kick(bpm, seconds, rate)` | A kick on every beat from sample 0: a sine dropping from 120 to 50 Hz with a fast fade (and a short tail fade, so no click), so energy jumps exactly every `60 / bpm` seconds |
| `chords(key, bpm, seconds, rate)` | I, IV, V, I in the key (i, iv, v, i in minor), one chord per bar of 4 beats (changes on bar lines), from middle C up; all 7 scale notes and only those (uses `Key::scale()` from `analysis::key`) |
| `bass(key, bpm, seconds, rate)` | The key's root two octaves below middle C on every off-beat |
| `test_loop(bpm, key, seconds, rate)` | The SPEC acceptance loop (Module 2): kick, bass and chords mixed and normalized to a -1 dBFS peak |

## Helpers

| Function | Purpose |
| --- | --- |
| `mix(parts)` | Sums sources; as long as the longest |
| `normalize(samples, peak_db)` | Scales to a peak level; silence stays silent |
| `peak_db(samples)` / `rms_db(samples)` | Levels in dBFS (negative infinity for silence) |
| `tone_power(samples, freq_hz, rate)` | Power of one frequency (Goertzel), length-normalized. Measure one steady stretch at a time (one chord): across note restarts, phases add or cancel |

## Tests

Sine level within 0.01 dB and frequency within 1 Hz; RMS of a full-scale sine is about -3 dBFS; kicks start on every beat (within one 10 ms window) at 92, 124, 128 and 140 BPM with the right count; bass notes start on every off-beat at the root pitch; chord changes fall on bar lines; chords in C major, A minor, F# major and D# minor (at 120 and 124 BPM, measured bar by bar) have in-key notes at least 10 times stronger than any other note, the tonic or fifth strongest, and the tonic stronger than the relative key's tonic; the test loop is full length, finite, peaks at -1 dBFS and keeps every kick as an onset; mix, normalize and invalid inputs (zero, negative, NaN, infinite or absurd BPM, negative length, empty input) give silence instead of a panic or a hang. The module doc example runs as a doc test (`--all-features`).
