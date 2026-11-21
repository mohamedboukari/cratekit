# Mastering (`cratekit-core::mastering`)

Mastering (SPEC, Module 3): presets, a loudness target, A/B and WAV export. The chain and the preset table are in `docs/ARCHITECTURE.md`, Mastering chain. This page covers the code.

So far: the DSP blocks (#23), and the presets with their intensity (#24). Loudness targeting and export (#25), the live preview with A/B (#26), reference matching (#27) and the Master tab (#28) come next.

Every block processes planar `f32` in place (one slice per channel, any type that is `AsMut<[f32]>`, so `Vec<f32>` or `&mut [f32]`) and never allocates while processing: buffers are sized when the block is made. So the live preview can run the same code in the audio callback.

## EQ (`mastering::eq`)

Biquad filters with the formulas of Robert Bristow-Johnson's "Audio EQ Cookbook" (a public formula reference).

| Item | What it is |
| --- | --- |
| `Coefficients::peaking(rate, freq, gain_db, q)` | A bell of `gain_db` at `freq` |
| `Coefficients::low_shelf` / `high_shelf(rate, freq, gain_db, slope)` | A shelf whose midpoint is `freq` (half the gain there); `slope` 1.0 is the steepest without overshoot |
| `Coefficients::low_pass` / `high_pass(rate, freq, q)` | 12 dB per octave; `q` 0.707 is flat, -3 dB at `freq` |
| `Coefficients::identity()` | Leaves the signal unchanged |
| `Coefficients::response_db(rate, freq)` | The filter's gain at a frequency (tests; later the EQ curve in the UI) |
| `Biquad::new(coefficients, channels)` | One filter per channel, transposed direct form II; `process`, `set` (new coefficients, same state), `reset` |

Coefficients and state are `f64`: the low shelves (70 to 120 Hz) sit close to the unit circle, where `f32` state adds noise. Out-of-range settings are clamped (frequency between 10 Hz and 0.49 of the rate, Q at least 0.05, shelf slope from 0.05 to 1, since steeper shelves resonate and can turn unstable); state below 1e-30 is flushed to zero so silent tails never run on denormals, and a sample that is not a number is taken as silence so it cannot poison the state.

## Compressor (`mastering::compressor`)

The glue compressor: `Compressor::new(rate, CompressorSettings { threshold_db, ratio, attack_ms, release_ms, knee_db, makeup_db })`, then `process`.

- **Detector:** RMS over all channels together (a 10 ms one-pole on the mean square), so both channels always get the same gain and the stereo image never shifts. A full-scale sine is about -3 dB RMS.
- **Curve:** no change below the knee; `(level - threshold) * (1 / ratio - 1)` above it; a smooth quadratic across the knee (`knee_db` wide, 6 dB by default).
- **Timing:** the gain moves toward the curve with the attack time when it must go down and the release time when it comes back.
- `reduction_db()` gives the current reduction for a meter. Ratio below 1 and negative knee are clamped; a sample that is not a number counts as silence in the detector.

## Limiter (`mastering::limiter`)

The look-ahead true-peak limiter at the end of the chain: `limit(channels, rate, LimiterSettings::default())` limits a whole signal in place, lined up with the input; `Limiter` is the streaming form (`process_frame`, `process`), whose output lags by `latency()` samples.

- **Ceiling:** `ceiling_dbtp`, -1.0 dBTP by default (the SPEC's limit), as a BS.1770 true-peak meter measures it. It aims 0.15 dB lower, for the gain moving between samples.
- **True peak:** each channel is interpolated between samples twice, and the higher estimate wins: with our own windowed-sinc filter (4 points, 24 taps per point, Kaiser window, each point's weights summing to 1; accurate up to about 0.42 of the rate), and the way a BS.1770 meter does it (48-tap Hann-windowed sinc, 4x below 96 kHz, 2x up to 192 kHz, none above; the formula `ebur128` uses, which measures our exports). Our filter alone let broadband transients (hi-hats, noise, a high tone starting at full level) read up to 0.14 dB over the ceiling on the meter. A sample's peak is the higher of the intervals on its two sides. On a quarter-rate sine whose samples sit at 0.707, both find the true peak at each point between samples.
- **Gain:** the need (`ceiling / peak`, or 1) is held at its minimum over the look-ahead window (5 ms by default; a monotonic queue), then averaged over that same window: so the gain is already down when a peak arrives, and every sample's gain is at or under what its own peak needs. When less reduction is needed, it comes back with the release time (80 ms by default).
- `gain()` gives the current gain for a meter. Up to 8 channels; a frame has as many samples as the limiter was made for (extra ones are left as they are). Settings are brought into range (look-ahead 0.1 to 50 ms; a ceiling or time that is not a number takes its default), and a sample that is not a number comes out as silence.

Speed (release build, M4 Pro): 8 s of stereo through the limiter, with the test signal made and measured, in about 86 ms, about 90 times faster than real time.

## Presets (`mastering::presets`)

The five presets of SPEC Module 3 with the starting values of the table in `docs/ARCHITECTURE.md`, Mastering chain (to tune by ear with the test DJ).

| Item | What it is |
| --- | --- |
| `PresetName` | `club`, `hip-hop`, `pop`, `lo-fi`, `chill` (as the UI sends them); `ALL` in the Master tab's order; `preset()`, and English `label()` and `description()` (the UI words known presets itself) |
| `Preset` | Low shelf, mid peak, high shelf (`Shelf`, `Peak`), an optional low pass (Lo-fi's 5.5 kHz), the compressor settings (soft knee 6 dB, no makeup: loudness targeting sets the level), the default target, and the intensity |
| `Preset::at(intensity)` | The preset at 0 to 1 (the SPEC's 0 to 100 %), always computed from the full preset: EQ gains times the intensity, ratio `1 + (ratio - 1) * intensity`; at 0 the EQ and compressor change nothing. Out of range is clamped; not a number is the full preset. **The loudness target never moves:** intensity is how much the EQ and compression colour the sound, and the master lands on the target chosen (decided 2026-11-21) |
| `PresetChain::new(rate, channels, &preset)` | The preset's EQ then compressor on a signal (`process`), four filters always (the fourth is the low pass, or a filter that changes nothing). The low pass slides on a log scale from 0.49 of the rate (intensity 0) to its corner (1), so a touch of intensity barely touches the audible band at any rate. `set(&preset)` switches preset or intensity keeping the filter and compressor state (for the live preview, #26); `eq_response_db(freq)` (the curve) and `reduction_db()` (a meter). Never allocates while processing. Extra channels beyond the count are compressed but not filtered |

The compressor gained `set(rate, settings)` (new settings, same state) and flushes its detector and gain to zero below 1e-30, so digital silence never runs on denormals.

In the app, `master_presets` (src-tauri `mastering.rs`) lists the presets for the Master tab: name, label, default target and description.

## Tests

All with synthetic audio (`crate::synth`):

- **EQ:** the identity is exact; a peak reaches its gain at its frequency and leaves far frequencies alone, from the formula and measured on sines; shelves give their gain on their side, half of it at the midpoint, nothing on the other side; passes are -3 dB at the corner and fall steeply above (or below) it; every filter goes silent after an impulse; channels keep their own state; out-of-range settings stay finite.
- **Compressor:** the static curve (ratio, knee, edges, ratio 1); a quiet signal passes unchanged; a steady -6 dBFS sine over a -20 dB threshold at 4:1 loses `10.97 * 3/4` dB; makeup adds exactly; attack is quick and release slow; a loud left channel and a quiet right one get the same gain.
- **EQ, more:** a gentler slope gives less gain below the shelf, slopes past 1 are clamped, a Q of 2 peaks 6 dB at the corner; `set` keeps the state and `reset` clears it; a not-a-number sample does not poison the filter; a zero sample rate does not panic.
- **Compressor, more:** a not-a-number sample does not switch compression off.
- **Presets:** every preset holds its full row of the architecture table (bands, Q, low pass, compressor with knee and makeup, target), in the Master tab's order; names serialize as the UI sends them; intensity scales the EQ and the ratio, always from the full preset, never the target, and is clamped; the low pass slides from the top of the band, barely touching 15 kHz at 1 % intensity at 48 and 96 kHz; intensity 0 leaves the test loop untouched on both channels for every preset; Club at full gives +3 dB at 40 Hz, -2 dB at 400 Hz and +2.5 dB at 16 kHz, measured and from its own curve, and half intensity half the lift; Lo-fi takes more than 20 dB off 12 kHz and leaves the body; the compressor reduces a loud tone by the preset's ratio; `set` changes the curve to the new preset's and keeps the filters running. In the app: `master_presets` lists all five as the UI reads them.
- **Limiter:** both interpolators pass DC and find the peak at each point between samples (4x at 48 kHz, 2x at 96 kHz, our own alone at 192 kHz); a quiet signal comes out unchanged and in line; the test loop pushed 12 dB over full scale measures at or under -1.0 dBTP (with `analysis::loudness`, the `ebur128` meter) and not crushed; broadband transients at 44.1, 48 and 96 kHz (the loop with hats, hats alone, binary noise, high tones starting at full level, sparse spikes, also with a 2 ms look-ahead) stay under it; a sine whose true peak is 3 dB above its sample peaks is caught; high notes near Nyquist at 44.1 kHz stay under it; our own detector reads every output sample at the target or under it (an impulse, a burst, hats); the gain goes down in steps of at most 1 / look-ahead and releases over the release time; the gain recovers after a burst; odd settings and not-a-number input are handled; the stream lags by exactly its latency.
