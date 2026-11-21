# Mastering (`cratekit-core::mastering`)

Mastering (SPEC, Module 3): presets, a loudness target, A/B and WAV export. The chain and the preset table are in `docs/ARCHITECTURE.md`, Mastering chain. This page covers the code.

So far: the DSP blocks (#23). Presets and intensity (#24), loudness targeting and export (#25), the live preview with A/B (#26), reference matching (#27) and the Master tab (#28) come next.

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

Coefficients and state are `f64`: the low shelves (70 to 120 Hz) sit close to the unit circle, where `f32` state adds noise. Out-of-range settings are clamped (frequency between 10 Hz and 0.49 of the rate, Q and slope at least 0.05); state below 1e-30 is flushed to zero so silent tails never run on denormals.

## Compressor (`mastering::compressor`)

The glue compressor: `Compressor::new(rate, CompressorSettings { threshold_db, ratio, attack_ms, release_ms, knee_db, makeup_db })`, then `process`.

- **Detector:** RMS over all channels together (a 10 ms one-pole on the mean square), so both channels always get the same gain and the stereo image never shifts. A full-scale sine is about -3 dB RMS.
- **Curve:** no change below the knee; `(level - threshold) * (1 / ratio - 1)` above it; a smooth quadratic across the knee (`knee_db` wide, 6 dB by default).
- **Timing:** the gain moves toward the curve with the attack time when it must go down and the release time when it comes back.
- `reduction_db()` gives the current reduction for a meter. Ratio below 1 and negative knee are clamped.

## Limiter (`mastering::limiter`)

The look-ahead true-peak limiter at the end of the chain: `limit(channels, rate, LimiterSettings::default())` limits a whole signal in place, lined up with the input; `Limiter` is the streaming form (`process_frame`, `process`), whose output lags by `latency()` samples.

- **Ceiling:** `ceiling_dbtp`, -1.0 dBTP by default (the SPEC's limit). It aims 0.15 dB lower, for the gain moving between samples and for the difference between our interpolator and a meter's.
- **True peak:** each channel is interpolated 4 times between samples with our own windowed-sinc filter (24 taps per point, Kaiser window, each point's weights summing to 1, designed at start-up), and a sample's peak is the higher of the intervals on its two sides. On a quarter-rate sine whose samples sit at 0.707, it finds the true peak within 0.5%.
- **Gain:** the need (`ceiling / peak`, or 1) is held at its minimum over the look-ahead window (5 ms by default; a monotonic queue), then averaged over that same window: so the gain is already down when a peak arrives, and every sample's gain is at or under what its own peak needs. When less reduction is needed, it comes back with the release time (80 ms by default).
- `gain()` gives the current gain for a meter. Up to 8 channels.

Speed (release build, M4 Pro): 8 s of stereo through the limiter, with the test signal made and measured, in about 44 ms, far faster than real time.

## Tests

All with synthetic audio (`crate::synth`):

- **EQ:** the identity is exact; a peak reaches its gain at its frequency and leaves far frequencies alone, from the formula and measured on sines; shelves give their gain on their side, half of it at the midpoint, nothing on the other side; passes are -3 dB at the corner and fall steeply above (or below) it; every filter goes silent after an impulse; channels keep their own state; out-of-range settings stay finite.
- **Compressor:** the static curve (ratio, knee, edges, ratio 1); a quiet signal passes unchanged; a steady -6 dBFS sine over a -20 dB threshold at 4:1 loses `10.97 * 3/4` dB; makeup adds exactly; attack is quick and release slow; a loud left channel and a quiet right one get the same gain.
- **Limiter:** the interpolator passes DC and finds the peak between samples; a quiet signal comes out unchanged and in line; the test loop pushed 12 dB over full scale measures at or under -1.0 dBTP (with `analysis::loudness`, the `ebur128` meter) and not crushed; a sine whose true peak is 3 dB above its sample peaks is caught; high notes near Nyquist at 44.1 kHz stay under the ceiling; the gain recovers after a burst; the stream lags by exactly its latency.
