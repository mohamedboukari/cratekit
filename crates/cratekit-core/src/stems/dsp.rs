//! The signal processing around the htdemucs model (#31).
//!
//! Intel's split export runs only the network: the STFT, its inverse, the
//! normalizations and the chunking of a whole track happen here, following
//! the reference implementation (adefossez/demucs: `spec.py`, `htdemucs.py`
//! `_spec`, `_ispec`, `forward`, and `apply.py`, MIT). See
//! docs/ARCHITECTURE.md, Stems notes, for the layout.
//!
//! A track is normalized as a whole (mean and standard deviation of its mono
//! mix), cut into segments of 7.8 s that overlap by a quarter, and each
//! segment is turned into the model's two inputs: the spectrogram (complex
//! values as channels) and the waveform, each normalized again. The model's
//! two outputs are turned back into audio, summed per source, and the
//! segments are blended with triangular weights.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// The model's sample rate.
pub const SAMPLE_RATE: u32 = 44_100;
/// Samples in one segment (7.8 s at 44.1 kHz), the model's fixed input size.
pub const SEGMENT: usize = 343_980;
/// FFT size.
pub const N_FFT: usize = 4096;
/// Hop between STFT frames.
pub const HOP: usize = N_FFT / 4;
/// Frequency bins the model sees (the Nyquist bin is dropped).
pub const FREQS: usize = N_FFT / 2;
/// STFT frames the model sees per segment.
pub const FRAMES: usize = SEGMENT.div_ceil(HOP);
/// Sources the model separates, in its order: drums, bass, other, vocals.
pub const SOURCES: usize = 4;
/// Audio channels (stereo).
pub const CHANNELS: usize = 2;
/// Step between segments: three quarters of a segment (25% overlap).
pub const STRIDE: usize = SEGMENT * 3 / 4;
/// The model's own padding around a segment before the STFT.
const PAD: usize = HOP / 2 * 3;

/// The periodic Hann window (as PyTorch's `hann_window`).
fn hann(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let phase = 2.0 * std::f64::consts::PI * i as f64 / len as f64;
            #[expect(clippy::cast_possible_truncation, reason = "a window value in 0 to 1")]
            let value = (0.5 - 0.5 * phase.cos()) as f32;
            value
        })
        .collect()
}

/// `x` with `left` and `right` samples mirrored around its ends, the edge
/// sample not repeated (PyTorch's "reflect"). Needs `left` and `right`
/// smaller than `x.len()`.
fn reflect_pad(x: &[f32], left: usize, right: usize) -> Vec<f32> {
    let n = x.len();
    let mut out = Vec::with_capacity(left + n + right);
    out.extend((1..=left).rev().map(|k| x.get(k).copied().unwrap_or(0.0)));
    out.extend_from_slice(x);
    out.extend((1..=right).map(|k| x.get(n.saturating_sub(1 + k)).copied().unwrap_or(0.0)));
    out
}

/// The STFT and its inverse as the model needs them, with the buffers kept
/// between calls.
pub struct Stft {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    window: Vec<f32>,
    frame: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scale: f32,
}

impl std::fmt::Debug for Stft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stft")
            .field("n_fft", &N_FFT)
            .finish_non_exhaustive()
    }
}

impl Default for Stft {
    fn default() -> Self {
        Self::new()
    }
}

impl Stft {
    /// Plans the FFTs (once; reuse the value for every segment).
    #[must_use]
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(N_FFT);
        let inverse = planner.plan_fft_inverse(N_FFT);
        let spectrum = forward.make_output_vec();
        #[expect(clippy::cast_precision_loss, reason = "N_FFT is 4096, exact in f32")]
        let scale = 1.0 / (N_FFT as f32).sqrt();
        Self {
            forward,
            inverse,
            window: hann(N_FFT),
            frame: vec![0.0; N_FFT],
            spectrum,
            scale,
        }
    }

    /// One channel of a segment (`SEGMENT` samples) as the model's
    /// spectrogram: `FREQS` x `FRAMES` complex values, frequency first
    /// (index `f * FRAMES + t`). The `_spec` of the reference.
    #[must_use]
    pub fn spec(&mut self, x: &[f32]) -> Vec<Complex<f32>> {
        let right = PAD + FRAMES * HOP - x.len().min(FRAMES * HOP);
        let padded = reflect_pad(x, PAD, right);
        // The STFT's own centring (n_fft / 2 on each side, reflected).
        let centred = reflect_pad(&padded, N_FFT / 2, N_FFT / 2);
        let mut out = vec![Complex::new(0.0, 0.0); FREQS * FRAMES];
        // The reference drops the first two and the last two frames.
        for t in 0..FRAMES {
            let start = (t + 2) * HOP;
            let source = centred.get(start..start + N_FFT).unwrap_or_default();
            for ((frame, &sample), &w) in self.frame.iter_mut().zip(source).zip(&self.window) {
                *frame = sample * w;
            }
            if self
                .forward
                .process(&mut self.frame, &mut self.spectrum)
                .is_err()
            {
                continue;
            }
            for (f, bin) in self.spectrum.iter().take(FREQS).enumerate() {
                if let Some(cell) = out.get_mut(f * FRAMES + t) {
                    *cell = bin * self.scale;
                }
            }
        }
        out
    }

    /// The model's spectrogram of one channel back to `length` samples (at
    /// most a segment). The `_ispec` of the reference: the dropped Nyquist bin
    /// and the dropped frames come back as zeros.
    #[must_use]
    pub fn ispec(&mut self, z: &[Complex<f32>], length: usize) -> Vec<f32> {
        let frames = FRAMES + 4;
        let total = (frames - 1) * HOP + N_FFT;
        let mut sum = vec![0.0_f32; total];
        let mut envelope = vec![0.0_f32; total];
        for t in 0..frames {
            for (f, bin) in self.spectrum.iter_mut().enumerate() {
                *bin = if (2..2 + FRAMES).contains(&t) && f < FREQS {
                    z.get(f * FRAMES + t - 2).copied().unwrap_or_default()
                } else {
                    Complex::new(0.0, 0.0)
                };
            }
            // The inverse real FFT needs real DC and Nyquist values.
            if let Some(dc) = self.spectrum.first_mut() {
                dc.im = 0.0;
            }
            if self
                .inverse
                .process(&mut self.spectrum, &mut self.frame)
                .is_err()
            {
                continue;
            }
            let start = t * HOP;
            let (Some(sum), Some(envelope)) = (
                sum.get_mut(start..start + N_FFT),
                envelope.get_mut(start..start + N_FFT),
            ) else {
                continue;
            };
            for (((acc, env), &value), &w) in sum
                .iter_mut()
                .zip(envelope.iter_mut())
                .zip(&self.frame)
                .zip(&self.window)
            {
                *acc += value * self.scale * w;
                *env += w * w;
            }
        }
        // Undo the centring (n_fft / 2), then the model's padding.
        let begin = N_FFT / 2 + PAD;
        (begin..begin + length)
            .map(|i| match (sum.get(i), envelope.get(i)) {
                (Some(&value), Some(&env)) if env > 1e-11 => value / env,
                _ => 0.0,
            })
            .collect()
    }
}

/// Mean and standard deviation (with Bessel's correction, as PyTorch's
/// `std`) of all the values.
fn mean_std<'a>(values: impl Iterator<Item = &'a f32> + Clone) -> (f32, f32) {
    let (mut count, mut sum) = (0_usize, 0.0_f64);
    for &v in values.clone() {
        count += 1;
        sum += f64::from(v);
    }
    if count < 2 {
        return (0.0, 0.0);
    }
    #[expect(clippy::cast_precision_loss, reason = "counts of samples fit f64")]
    let mean = sum / count as f64;
    let squares: f64 = values.map(|&v| (f64::from(v) - mean).powi(2)).sum();
    #[expect(clippy::cast_precision_loss, reason = "counts of samples fit f64")]
    let std = (squares / (count - 1) as f64).sqrt();
    #[expect(clippy::cast_possible_truncation, reason = "audio statistics fit f32")]
    let stats = (mean as f32, std as f32);
    stats
}

/// The mean and spread a track was normalized with (undo with [`Self::undo`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackScale {
    mean: f32,
    std: f32,
}

impl TrackScale {
    /// Normalizes a track in place by the mean and standard deviation of its
    /// mono mix, as the reference does before separating.
    #[must_use]
    pub fn normalize(channels: &mut [Vec<f32>]) -> Self {
        let frames = channels.iter().map(Vec::len).min().unwrap_or(0);
        #[expect(clippy::cast_precision_loss, reason = "channel counts are tiny")]
        let count = channels.len().max(1) as f32;
        let mono: Vec<f32> = (0..frames)
            .map(|i| channels.iter().filter_map(|c| c.get(i)).sum::<f32>() / count)
            .collect();
        let (mean, std) = mean_std(mono.iter());
        let scale = Self {
            mean,
            std: std + 1e-8,
        };
        for sample in channels.iter_mut().flatten() {
            *sample = (*sample - scale.mean) / scale.std;
        }
        scale
    }

    /// Undoes [`Self::normalize`] on separated audio.
    pub fn undo(&self, channels: &mut [Vec<f32>]) {
        for sample in channels.iter_mut().flatten() {
            *sample = *sample * self.std + self.mean;
        }
    }
}

/// The model's two inputs for one segment, and what undoes their
/// normalization.
#[derive(Debug, Clone)]
pub struct ModelInput {
    /// The spectrogram, complex values as channels: `[1, 4, FREQS, FRAMES]`
    /// (channel 0 real, channel 0 imaginary, channel 1 real, channel 1
    /// imaginary), normalized.
    pub spectrogram: Vec<f32>,
    /// The waveform, `[1, 2, SEGMENT]`, normalized.
    pub waveform: Vec<f32>,
    spec_stats: (f32, f32),
    wave_stats: (f32, f32),
}

/// Builds the model's inputs for one stereo segment of `SEGMENT` samples
/// (the `forward` of the reference, up to the network).
#[must_use]
pub fn model_input(stft: &mut Stft, segment: &[Vec<f32>; CHANNELS]) -> ModelInput {
    let mut spectrogram = Vec::with_capacity(2 * CHANNELS * FREQS * FRAMES);
    for channel in segment {
        let z = stft.spec(channel);
        spectrogram.extend(z.iter().map(|c| c.re));
        spectrogram.extend(z.iter().map(|c| c.im));
    }
    let spec_stats = mean_std(spectrogram.iter());
    let (mean, std) = spec_stats;
    for v in &mut spectrogram {
        *v = (*v - mean) / (1e-5 + std);
    }
    let mut waveform: Vec<f32> = segment.iter().flatten().copied().collect();
    let wave_stats = mean_std(waveform.iter());
    let (mean, std) = wave_stats;
    for v in &mut waveform {
        *v = (*v - mean) / (1e-5 + std);
    }
    ModelInput {
        spectrogram,
        waveform,
        spec_stats,
        wave_stats,
    }
}

/// Turns the model's two outputs for one segment into audio: per source
/// (drums, bass, other, vocals), two channels of `SEGMENT` samples. The end
/// of the reference's `forward`: undo both normalizations, the inverse STFT
/// of the spectrogram branch, plus the waveform branch.
#[must_use]
pub fn model_output(
    stft: &mut Stft,
    input: &ModelInput,
    spectrogram: &[f32],
    waveform: &[f32],
) -> Vec<[Vec<f32>; CHANNELS]> {
    let plane = FREQS * FRAMES;
    let (spec_mean, spec_std) = input.spec_stats;
    let (wave_mean, wave_std) = input.wave_stats;
    (0..SOURCES)
        .map(|source| {
            std::array::from_fn(|channel| {
                let base = (source * CHANNELS + channel) * 2 * plane;
                let re = spectrogram.get(base..base + plane).unwrap_or_default();
                let im = spectrogram
                    .get(base + plane..base + 2 * plane)
                    .unwrap_or_default();
                let z: Vec<Complex<f32>> = re
                    .iter()
                    .zip(im)
                    .map(|(&r, &i)| {
                        Complex::new(r * spec_std + spec_mean, i * spec_std + spec_mean)
                    })
                    .collect();
                let mut audio = stft.ispec(&z, SEGMENT);
                let start = (source * CHANNELS + channel) * SEGMENT;
                let time = waveform.get(start..start + SEGMENT).unwrap_or_default();
                for (sample, &t) in audio.iter_mut().zip(time) {
                    *sample += t * wave_std + wave_mean;
                }
                audio
            })
        })
        .collect()
}

/// One segment of a track: where its result goes, and where the model's
/// window over the track starts (a short last segment is centred in a full
/// window, with the track's own samples around it when there are any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    /// First frame of the track this segment produces.
    pub offset: usize,
    /// How many frames it produces (`SEGMENT`, or fewer at the end).
    pub len: usize,
    /// Where the model's `SEGMENT`-long window starts (may be before 0).
    start: isize,
}

/// The segments of a track of `frames` frames (offsets every `STRIDE`).
#[must_use]
pub fn chunks(frames: usize) -> Vec<Chunk> {
    (0..frames)
        .step_by(STRIDE)
        .map(|offset| {
            let len = SEGMENT.min(frames - offset);
            let delta = SEGMENT - len;
            #[expect(clippy::cast_possible_wrap, reason = "track lengths fit isize")]
            let start = offset as isize - (delta / 2) as isize;
            Chunk { offset, len, start }
        })
        .collect()
}

impl Chunk {
    /// The model's input window: `SEGMENT` frames of each channel, zeros
    /// outside the track.
    #[must_use]
    pub fn window(&self, track: &[Vec<f32>]) -> [Vec<f32>; CHANNELS] {
        std::array::from_fn(|channel| {
            let samples = track.get(channel).or_else(|| track.first());
            (0..SEGMENT)
                .map(|i| {
                    #[expect(clippy::cast_possible_wrap, reason = "SEGMENT fits isize")]
                    let at = self.start + i as isize;
                    usize::try_from(at)
                        .ok()
                        .and_then(|at| samples.and_then(|s| s.get(at)))
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect()
        })
    }

    /// The part of a model output that belongs to this segment.
    fn trim<'a>(&self, output: &'a [f32]) -> &'a [f32] {
        let skip = (SEGMENT - self.len) / 2;
        output.get(skip..skip + self.len).unwrap_or_default()
    }
}

/// The triangular weight of a segment's samples, highest in the middle.
#[must_use]
pub fn segment_weight() -> Vec<f32> {
    let half = SEGMENT / 2;
    #[expect(
        clippy::cast_precision_loss,
        reason = "segment positions fit f32 exactly"
    )]
    let weight: Vec<f32> = (1..=half)
        .chain((1..=SEGMENT - half).rev())
        .map(|w| w as f32 / half as f32)
        .collect();
    weight
}

/// Blends the segments' outputs into whole sources.
#[derive(Debug, Clone)]
pub struct Blend {
    /// Per source, per channel, the weighted sum.
    sources: Vec<[Vec<f32>; CHANNELS]>,
    total_weight: Vec<f32>,
    weight: Vec<f32>,
}

impl Blend {
    /// An empty blend for a track of `frames` frames.
    #[must_use]
    pub fn new(frames: usize) -> Self {
        Self {
            sources: (0..SOURCES)
                .map(|_| std::array::from_fn(|_| vec![0.0; frames]))
                .collect(),
            total_weight: vec![0.0; frames],
            weight: segment_weight(),
        }
    }

    /// Adds one segment's output (per source, two channels of `SEGMENT`).
    pub fn add(&mut self, chunk: &Chunk, output: &[[Vec<f32>; CHANNELS]]) {
        let range = chunk.offset..chunk.offset + chunk.len;
        let weight = self.weight.get(..chunk.len).unwrap_or_default();
        for (sum, &w) in self
            .total_weight
            .get_mut(range.clone())
            .unwrap_or_default()
            .iter_mut()
            .zip(weight)
        {
            *sum += w;
        }
        for (source, separated) in self.sources.iter_mut().zip(output) {
            for (channel, part) in source.iter_mut().zip(separated) {
                let target = channel.get_mut(range.clone()).unwrap_or_default();
                for ((acc, &value), &w) in target.iter_mut().zip(chunk.trim(part)).zip(weight) {
                    *acc += value * w;
                }
            }
        }
    }

    /// The sources (drums, bass, other, vocals), each two channels.
    #[must_use]
    pub fn finish(mut self) -> Vec<[Vec<f32>; CHANNELS]> {
        for source in &mut self.sources {
            for channel in source.iter_mut() {
                for (value, &w) in channel.iter_mut().zip(&self.total_weight) {
                    if w > 0.0 {
                        *value /= w;
                    }
                }
            }
        }
        self.sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, amplitude: f64, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                (amplitude
                    * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(SAMPLE_RATE)).sin())
                    as f32
            })
            .collect()
    }

    #[test]
    fn the_constants_match_the_reference() {
        assert_eq!(FRAMES, 336);
        assert_eq!(STRIDE, 257_985);
        assert_eq!(PAD, 1536);
    }

    #[test]
    fn reflect_padding_mirrors_without_repeating_the_edge() {
        assert_eq!(
            reflect_pad(&[1.0, 2.0, 3.0, 4.0], 2, 2),
            vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]
        );
    }

    #[test]
    fn the_window_is_the_periodic_hann() {
        let w = hann(8);
        assert!(w[0].abs() < 1e-7);
        assert!((w[4] - 1.0).abs() < 1e-7);
        assert!((w[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_tone_lands_in_its_bin_with_the_normalized_level() {
        // Bin 100 exactly: f = 100 * 44100 / 4096.
        let freq = 100.0 * f64::from(SAMPLE_RATE) / N_FFT as f64;
        let mut stft = Stft::new();
        let z = stft.spec(&sine(freq, 0.5, SEGMENT));
        let t = FRAMES / 2;
        let magnitude = |f: usize| z[f * FRAMES + t].norm();
        // A Hann-windowed sine of amplitude A: A * N / 4, scaled by 1 / sqrt(N).
        let expected = 0.5 * N_FFT as f32 / 4.0 / (N_FFT as f32).sqrt();
        assert!(
            (magnitude(100) - expected).abs() < 0.01 * expected,
            "{}",
            magnitude(100)
        );
        assert!(magnitude(110) < 1e-3 * expected);
    }

    #[test]
    fn the_inverse_gives_the_signal_back() {
        let x: Vec<f32> = sine(440.0, 0.3, SEGMENT)
            .iter()
            .zip(sine(5000.0, 0.2, SEGMENT))
            .map(|(a, b)| a + b)
            .collect();
        let mut stft = Stft::new();
        let z = stft.spec(&x);
        let y = stft.ispec(&z, SEGMENT);
        // The reference drops two frames at each end, so the first and last
        // few thousand samples are attenuated; everything else comes back.
        let inner = 4 * HOP..SEGMENT - 4 * HOP;
        let error = inner
            .clone()
            .map(|i| (x[i] - y[i]).abs())
            .fold(0.0_f32, f32::max);
        assert!(error < 1e-4, "max error {error}");
    }

    #[test]
    fn segments_cover_the_track_and_centre_the_last_one() {
        let frames = SEGMENT * 2 + 1000;
        let all = chunks(frames);
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].offset, STRIDE);
        let last = all[2];
        assert_eq!(last.offset + last.len, frames);
        assert_eq!(
            last.start,
            last.offset as isize - ((SEGMENT - last.len) / 2) as isize
        );
        // A track shorter than a segment: one centred window.
        let short = chunks(1000);
        assert_eq!(short.len(), 1);
        assert!(short[0].start < 0);
    }

    #[test]
    fn blending_an_identity_model_gives_the_track_back() {
        let frames = SEGMENT * 2 + 12_345;
        let track = vec![sine(220.0, 0.4, frames), sine(330.0, 0.3, frames)];
        let mut blend = Blend::new(frames);
        for chunk in chunks(frames) {
            let window = chunk.window(&track);
            // Every source gets the input: the blend must undo the weights.
            let output: Vec<[Vec<f32>; CHANNELS]> = (0..SOURCES).map(|_| window.clone()).collect();
            blend.add(&chunk, &output);
        }
        let sources = blend.finish();
        for source in &sources {
            for (channel, original) in source.iter().zip(&track) {
                let error = channel
                    .iter()
                    .zip(original)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(error < 1e-5, "max error {error}");
            }
        }
    }

    #[test]
    fn normalizing_a_track_is_undone_exactly() {
        let original = vec![sine(100.0, 0.5, 5000), sine(150.0, 0.25, 5000)];
        let mut track = original.clone();
        let scale = TrackScale::normalize(&mut track);
        let (mean, std) = mean_std(
            track
                .iter()
                .flatten()
                .collect::<Vec<_>>()
                .into_iter()
                .copied()
                .collect::<Vec<f32>>()
                .iter(),
        );
        assert!(mean.abs() < 1e-3 && std > 0.5, "{mean} {std}");
        scale.undo(&mut track);
        for (a, b) in track.iter().flatten().zip(original.iter().flatten()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn the_spectrogram_input_puts_real_then_imaginary_per_channel() {
        let mut stft = Stft::new();
        let left = sine(1000.0, 0.5, SEGMENT);
        let input = model_input(&mut stft, &[left.clone(), vec![0.0; SEGMENT]]);
        assert_eq!(input.spectrogram.len(), 4 * FREQS * FRAMES);
        assert_eq!(input.waveform.len(), 2 * SEGMENT);
        let plane = FREQS * FRAMES;
        let (mean, std) = input.spec_stats;
        // Channel 1 is silent: its two planes hold only the normalized zero.
        let zero = (0.0 - mean) / (1e-5 + std);
        assert!(
            input.spectrogram[2 * plane..]
                .iter()
                .all(|&v| (v - zero).abs() < 1e-6)
        );
        assert!(
            input.spectrogram[..plane]
                .iter()
                .any(|&v| (v - zero).abs() > 1.0)
        );
    }

    #[test]
    fn the_output_turns_both_branches_back_into_audio() {
        let mut stft = Stft::new();
        let left = sine(440.0, 0.3, SEGMENT);
        let right = sine(660.0, 0.2, SEGMENT);
        let input = model_input(&mut stft, &[left.clone(), right.clone()]);
        // A model that puts the whole mix into source 3 (vocals), spectrogram
        // branch only: the output must be the mix there and silence elsewhere.
        let plane = 2 * FREQS * FRAMES;
        let (spec_mean, spec_std) = input.spec_stats;
        let silent_spec = -spec_mean / spec_std;
        let mut spectrogram = vec![silent_spec; SOURCES * CHANNELS * plane];
        spectrogram[3 * CHANNELS * plane..].copy_from_slice(&input.spectrogram);
        let (wave_mean, wave_std) = input.wave_stats;
        let waveform = vec![-wave_mean / wave_std; SOURCES * CHANNELS * SEGMENT];
        let sources = model_output(&mut stft, &input, &spectrogram, &waveform);
        let inner = 4 * HOP..SEGMENT - 4 * HOP;
        for (channel, original) in sources[3].iter().zip([&left, &right]) {
            let error = inner
                .clone()
                .map(|i| (channel[i] - original[i]).abs())
                .fold(0.0_f32, f32::max);
            assert!(error < 1e-3, "vocals error {error}");
        }
        let leak = inner
            .map(|i| sources[0][0][i].abs())
            .fold(0.0_f32, f32::max);
        assert!(leak < 1e-3, "drums {leak}");
    }
}
