//! Tempo (BPM) detection (SPEC, Module 2).
//!
//! [`TempoDetector`] is the interface the rest of Cratekit uses, so the
//! implementation can be swapped (`stratum-dsp` is the second one, behind
//! the `stratum` feature: see `analysis::stratum`). [`OnsetTempo`]
//! is our own: it finds where hits start (an onset envelope), estimates the
//! beat period from how that envelope repeats (autocorrelation), then refines
//! the tempo by lining a beat grid up with every onset in the clip.

/// The tempo of a piece of audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoEstimate {
    /// Beats per minute, folded into 70 to 180 (the half/double-time fix).
    pub bpm: f32,
    /// How clearly the audio has a steady beat, 0 to 1.
    pub confidence: f32,
    /// When the first beat falls, in seconds from the start of the samples
    /// given, within one beat period (`0 <= first_beat_sec < 60 / bpm`).
    /// Together with `bpm` it places every beat (the beat grid). `None` when
    /// the detector does not find beat positions.
    pub first_beat_sec: Option<f32>,
}

/// Finds the tempo of mono audio.
///
/// Implementations analyze whatever they are given; choosing which part of a
/// track to analyze is the caller's job.
pub trait TempoDetector {
    /// The tempo of `samples` (mono, `sample_rate` Hz), or `None` when there
    /// is no usable beat: silence, too short, or invalid samples.
    fn detect_tempo(&self, samples: &[f32], sample_rate: u32) -> Option<TempoEstimate>;
}

/// The SPEC range for reported tempos: anything outside is halved or doubled.
pub const BPM_RANGE: (f32, f32) = (70.0, 180.0);

/// Onset envelope resolution: one value every 5 ms.
pub(crate) const HOP_SECONDS: f64 = 0.005;
/// Length of each of the two box averages that smooth the energy (together a
/// 40 ms triangle): long enough that low bass notes and drones do not ripple
/// it, short enough to keep onsets sharp.
pub(crate) const WINDOW_SECONDS: f64 = 0.02;
/// Tempos the coarse search considers: wider than `BPM_RANGE` (results are
/// folded into it), with a margin so a beat near either end is still an
/// interior peak of the autocorrelation, not an edge value.
const SEARCH_BPM: (f64, f64) = (55.0, 210.0);
/// A shorter lag wins over the strongest one only when it scores at least this
/// share of it and divides it evenly: then both are the same hits seen one
/// or several beats apart, and the shorter one is the beat. Off-beat parts
/// (hi-hats on eighth notes) score clearly lower, so they never halve the
/// tempo.
const SHORTER_LAG_SHARE: f64 = 0.9;
/// How close (as a share of the lag) a shorter peak must be to a whole
/// division (1/2, 1/3, 1/4) of the strongest one to count as its beat.
const DIVISION_TOLERANCE: f64 = 0.04;
/// Weakest beat worth reporting: the average rise in log energy at the beats
/// (about a 15% energy jump). A steady tone still ripples a little in a 20 ms
/// window (up to about 13% for a 30 Hz drone); real hits rise far more.
const MIN_ONSET_STRENGTH: f64 = 0.15;
/// The fine search looks this far either side of the coarse tempo.
const FINE_SPAN: f64 = 0.03;
/// Moving-average width (frames) applied before the autocorrelation: 35 ms.
const SMOOTH_FRAMES: usize = 7;
/// Fine search step, in BPM.
const FINE_STEP_BPM: f64 = 0.01;
/// Phase step of the fine search, in envelope frames.
const PHASE_STEP: f64 = 0.5;
/// How long before the end of an envelope frame's window a hit's rise peaks.
/// Measured on full-level synthetic kicks at 92 to 140 BPM (first beats
/// within 2 ms of the true one). Quieter hits, or hits over a sustained pad,
/// rise less steeply and land up to about 12 ms late; re-check on the test
/// DJ's tracks (#22).
const ONSET_LATENCY_SECONDS: f64 = 0.006;
/// Shortest audio worth analyzing, in seconds.
pub(crate) const MIN_SECONDS: f64 = 3.0;

/// Our tempo detector: onset envelope, autocorrelation, then a fine beat-grid
/// fit over the whole clip.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnsetTempo;

impl TempoDetector for OnsetTempo {
    fn detect_tempo(&self, samples: &[f32], sample_rate: u32) -> Option<TempoEstimate> {
        if sample_rate == 0
            || (samples.len() as f64) < MIN_SECONDS * f64::from(sample_rate)
            || samples.iter().any(|s| !s.is_finite())
        {
            return None;
        }
        let hop = (f64::from(sample_rate) * HOP_SECONDS).round().max(1.0) as usize;
        let frame_seconds = hop as f64 / f64::from(sample_rate);
        let window = (f64::from(sample_rate) * WINDOW_SECONDS).round().max(1.0) as usize;
        let envelope = onset_envelope(samples, hop, window);
        if envelope.iter().all(|&v| v <= f64::EPSILON) {
            return None;
        }
        let coarse_bpm = coarse_bpm(&envelope, frame_seconds)?;
        let fit = refine_bpm(&envelope, frame_seconds, coarse_bpm);
        if fit.strength < MIN_ONSET_STRENGTH {
            return None;
        }
        let bpm = fold_into_range(fit.bpm);
        Some(TempoEstimate {
            bpm: bpm as f32,
            confidence: fit.confidence as f32,
            first_beat_sec: Some(first_beat(fit.phase_frames, frame_seconds, bpm) as f32),
        })
    }
}

/// How strongly a new sound starts in each frame (every `hop` samples): the
/// rise in (compressed) energy from the previous frame, and zero when the
/// energy falls.
pub(crate) fn onset_envelope(samples: &[f32], hop: usize, window: usize) -> Vec<f64> {
    onset_envelope_of(
        samples.len(),
        |n| f64::from(samples[n]).powi(2),
        hop,
        window,
    )
}

/// [`onset_envelope`] of any signal given as its power at each sample
/// (`power(n)` for `n` in `0..len`), so callers can derive a signal on the
/// fly (the energy score uses the sample-to-sample difference) without a
/// full-length copy.
pub(crate) fn onset_envelope_of(
    len: usize,
    power: impl Fn(usize) -> f64,
    hop: usize,
    window: usize,
) -> Vec<f64> {
    // Two box averages in a row make a triangle-shaped window twice as long
    // (about 40 ms). A steady tone's energy pulses at twice its frequency; a
    // single box barely smooths that, two boxes leave less than 5% of it for
    // any tone above about 25 Hz, so drones and pads do not look like beats.
    // The first box is sampled every millisecond (`fine`), then the second
    // averages those over the same length, ending at each hop.
    let fine = (hop / 5).max(1);
    // The first box as a sliding sum: samples are added as they enter the
    // window and subtracted as they leave, so nothing track-length is stored.
    let mut first = Vec::with_capacity(len / fine);
    let (mut sum, mut entered, mut left) = (0.0_f64, 0, 0);
    for end in (fine..=len).step_by(fine) {
        while entered < end {
            sum += power(entered);
            entered += 1;
        }
        let start = end.saturating_sub(window);
        while left < start {
            sum -= power(left);
            left += 1;
        }
        // Rounding can leave a hair below zero after a loud passage.
        first.push(sum.max(0.0) / (end - start) as f64);
    }
    let mut first_running = Vec::with_capacity(first.len() + 1);
    first_running.push(0.0_f64);
    let mut sum = 0.0;
    for &value in &first {
        sum += value;
        first_running.push(sum);
    }
    let span = (window / fine).max(1);
    // log(1 + 1000 * energy) puts loud and quiet hits on a similar scale and
    // stays near zero for near silence.
    let loudness: Vec<f64> = (hop..=len)
        .step_by(hop)
        .map(|end| {
            let last = (end / fine).min(first.len());
            let begin = last.saturating_sub(span);
            let energy = if last > begin {
                (first_running[last] - first_running[begin]) / (last - begin) as f64
            } else {
                0.0
            };
            (1000.0 * energy).ln_1p()
        })
        .collect();
    let mut envelope = vec![0.0; loudness.len()];
    for (slot, pair) in envelope.iter_mut().skip(1).zip(loudness.windows(2)) {
        *slot = (pair[1] - pair[0]).max(0.0);
    }
    envelope
}

/// The beat period from the envelope's autocorrelation, as a BPM.
///
/// Autocorrelation compares the envelope with a shifted copy of itself: a
/// shift of one beat lines the hits up again, so it scores high. Among the
/// high-scoring shifts, the shortest one is the beat (longer ones are
/// multiples of it).
fn coarse_bpm(envelope: &[f64], frame_seconds: f64) -> Option<f64> {
    // Smooth first: an onset is only a frame or two wide, so a beat period
    // that falls between whole frames would score lower than a multiple of it
    // that happens to land on one.
    let smooth = smoothed(envelope, SMOOTH_FRAMES);
    let mean = smooth.iter().sum::<f64>() / smooth.len() as f64;
    let centered: Vec<f64> = smooth.iter().map(|v| v - mean).collect();
    let shortest = (60.0 / SEARCH_BPM.1 / frame_seconds).floor() as usize;
    let longest = (60.0 / SEARCH_BPM.0 / frame_seconds).ceil() as usize;
    if longest + 1 >= centered.len() || shortest == 0 {
        return None;
    }
    let scores: Vec<f64> = (shortest..=longest)
        .map(|lag| {
            let overlap = centered.len() - lag;
            let sum: f64 = centered
                .iter()
                .zip(&centered[lag..])
                .map(|(a, b)| a * b)
                .sum();
            sum / overlap as f64
        })
        .collect();
    // Each local peak, refined between whole frames: the beat period rarely
    // falls exactly on a frame, so fit a parabola through the peak and its two
    // neighbours to estimate the true peak position and height.
    let peaks: Vec<(f64, f64)> = scores
        .windows(3)
        .enumerate()
        .filter(|(_, w)| w[1] >= w[0] && w[1] >= w[2] && w[1] > 0.0)
        .map(|(i, w)| {
            let (left, mid, right) = (w[0], w[1], w[2]);
            let curve = left - 2.0 * mid + right;
            let shift = if curve < 0.0 {
                (0.5 * (left - right) / curve).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let height = mid - 0.25 * (left - right) * shift;
            ((shortest + i + 1) as f64 + shift, height)
        })
        .collect();
    let best = peaks.iter().map(|&(_, h)| h).fold(0.0_f64, f64::max);
    if best <= 0.0 {
        return None;
    }
    let strongest = peaks
        .iter()
        .find(|&&(_, h)| h >= best)
        .map_or(shortest as f64, |&(l, _)| l);
    // A nearly-as-strong peak at a half, third or quarter of the strongest
    // lag is the real beat (the strongest one is then a whole multiple).
    let lag = [4.0, 3.0, 2.0]
        .iter()
        .filter_map(|&division| {
            let target = strongest / division;
            peaks.iter().find(|&&(l, h)| {
                (l - target).abs() <= DIVISION_TOLERANCE * target && h >= SHORTER_LAG_SHARE * best
            })
        })
        .map(|&(l, _)| l)
        .next()
        .unwrap_or(strongest);
    Some(60.0 / (lag * frame_seconds))
}

/// A moving average over `width` frames (centred), the same length as `values`.
fn smoothed(values: &[f64], width: usize) -> Vec<f64> {
    let half = width / 2;
    (0..values.len())
        .map(|i| {
            let start = i.saturating_sub(half);
            let end = (i + half + 1).min(values.len());
            values[start..end].iter().sum::<f64>() / (end - start) as f64
        })
        .collect()
}

/// The value of the envelope at a fractional frame position (straight-line
/// interpolation between frames; zero outside).
fn envelope_at(envelope: &[f64], position: f64) -> f64 {
    if position < 0.0 {
        return 0.0;
    }
    let index = position.floor() as usize;
    let fraction = position - index as f64;
    let here = envelope.get(index).copied().unwrap_or(0.0);
    let next = envelope.get(index + 1).copied().unwrap_or(0.0);
    here + (next - here) * fraction
}

/// How well a beat grid at `bpm` lines up with the onsets: the best average
/// envelope value at the grid's beats over every starting phase, the average
/// over all phases (what a grid in a random place scores), and the best
/// phase (in envelope frames).
fn grid_score(envelope: &[f64], frame_seconds: f64, bpm: f64) -> GridScore {
    let period = 60.0 / bpm / frame_seconds;
    let length = envelope.len() as f64;
    let mut best = 0.0_f64;
    let mut best_phase = 0.0;
    let (mut all_phases, mut phases) = (0.0, 0_u32);
    let mut phase = 0.0;
    while phase < period {
        let (mut sum, mut beats) = (0.0, 0_u32);
        let mut position = phase;
        while position < length {
            sum += envelope_at(envelope, position);
            beats += 1;
            position += period;
        }
        if beats > 0 {
            let score = sum / f64::from(beats);
            if score > best {
                best = score;
                best_phase = phase;
            }
            all_phases += score;
            phases += 1;
        }
        phase += PHASE_STEP;
    }
    GridScore {
        best,
        typical: all_phases / f64::from(phases.max(1)),
        phase: best_phase,
    }
}

/// What [`grid_score`] found for one tempo.
#[derive(Debug, Clone, Copy)]
struct GridScore {
    best: f64,
    typical: f64,
    phase: f64,
}

/// The winning beat grid of [`refine_bpm`].
#[derive(Debug, Clone, Copy)]
struct GridFit {
    bpm: f64,
    /// Average rise in log energy at the grid's beats.
    strength: f64,
    confidence: f64,
    /// The first beat, in envelope frames.
    phase_frames: f64,
}

/// The time of the first beat from the grid's phase, in seconds, within one
/// period of `bpm` (the folded tempo).
///
/// Envelope frame `k` measures the energy rise in the 40 ms window ending at
/// `(k + 1)` frames. Because the energy is compressed with a logarithm, a
/// hit's rise peaks early in that window: `ONSET_LATENCY_SECONDS` before its
/// end.
fn first_beat(phase_frames: f64, frame_seconds: f64, bpm: f64) -> f64 {
    let period = 60.0 / bpm;
    let time = (phase_frames + 1.0) * frame_seconds - ONSET_LATENCY_SECONDS;
    time.rem_euclid(period)
}

/// Refines the tempo near `coarse` by trying grids every 0.01 BPM within 3%.
/// A grid that matches every hit across the whole clip pins the tempo far
/// more precisely than one frame of the envelope (5 ms) could on its own.
///
/// Returns the tempo, how strong its beats are (average rise in log energy
/// at the grid), and a confidence: how far the best placement of the winning
/// grid scores above an average placement. That compares beats with the
/// space between them, so it does not depend on how long the clip is.
fn refine_bpm(envelope: &[f64], frame_seconds: f64, coarse: f64) -> GridFit {
    let low = coarse * (1.0 - FINE_SPAN);
    let steps = ((coarse * 2.0 * FINE_SPAN) / FINE_STEP_BPM).round() as u32;
    let (bpm, score) = (0..=steps)
        .map(|i| {
            let bpm = low + f64::from(i) * FINE_STEP_BPM;
            (bpm, grid_score(envelope, frame_seconds, bpm))
        })
        .max_by(|a, b| a.1.best.total_cmp(&b.1.best))
        .unwrap_or((
            coarse,
            GridScore {
                best: 0.0,
                typical: 0.0,
                phase: 0.0,
            },
        ));
    let confidence = if score.best > 0.0 {
        (1.0 - score.typical / score.best).clamp(0.0, 1.0)
    } else {
        0.0
    };
    GridFit {
        bpm,
        strength: score.best,
        confidence,
        phase_frames: score.phase,
    }
}

/// Halves or doubles `bpm` until it is inside [`BPM_RANGE`].
pub(crate) fn fold_into_range(mut bpm: f64) -> f64 {
    let (low, high) = (f64::from(BPM_RANGE.0), f64::from(BPM_RANGE.1));
    while bpm > high {
        bpm /= 2.0;
    }
    while bpm < low && bpm > 0.0 {
        bpm *= 2.0;
    }
    bpm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::synth;

    const RATE: u32 = 44_100;
    const TEMPOS: [f32; 4] = [92.0, 124.0, 128.0, 140.0];

    fn detect(samples: &[f32]) -> Option<TempoEstimate> {
        OnsetTempo.detect_tempo(samples, RATE)
    }

    #[test]
    fn places_the_first_beat() {
        // Kicks starting `offset` seconds in: the first beat is at `offset`
        // (or a whole beat later, folded into one period).
        for bpm in TEMPOS {
            let period = 60.0 / bpm;
            for offset in [0.0_f32, 0.017, 0.05, 0.123, 0.211, 0.3] {
                let mut samples = vec![0.0; (offset * RATE as f32) as usize];
                samples.extend(synth::kick(bpm, 10.0, RATE));
                let found = detect(&samples).unwrap();
                let first = found.first_beat_sec.unwrap();
                assert!((0.0..period).contains(&first), "{bpm}/{offset}: {first}");
                let expected = offset.rem_euclid(period);
                let error = (first - expected)
                    .abs()
                    .min(period - (first - expected).abs());
                assert!(
                    error <= 0.005,
                    "{bpm} BPM, offset {offset}: first beat {first}"
                );
            }
        }
    }

    #[test]
    fn places_the_first_beat_of_quieter_kicks_and_kicks_over_a_pad_a_little_later() {
        // The latency is calibrated on full-level kicks; quieter hits, or hits
        // over a sustained pad, rise less steeply and land up to about 12 ms
        // late (re-check on real tracks, #22).
        for (gain, pad) in [(0.25_f32, false), (0.1, false), (1.0, true)] {
            for bpm in TEMPOS {
                let period = 60.0 / bpm;
                for offset in [0.0_f32, 0.123, 0.3] {
                    let mut samples = vec![0.0; (offset * RATE as f32) as usize];
                    samples.extend(synth::kick(bpm, 10.0, RATE).iter().map(|s| s * gain));
                    if pad {
                        let seconds = samples.len() as f32 / RATE as f32;
                        let bed = synth::sine(220.0, -10.0, seconds, RATE);
                        samples = synth::mix(&[&samples, &bed]);
                    }
                    let first = detect(&samples).unwrap().first_beat_sec.unwrap();
                    let expected = offset.rem_euclid(period);
                    let error = (first - expected)
                        .abs()
                        .min(period - (first - expected).abs());
                    assert!(
                        error <= 0.012,
                        "gain {gain}, pad {pad}, {bpm} BPM, offset {offset}: {first}"
                    );
                }
            }
        }
    }

    #[test]
    fn finds_the_tempo_of_kicks_alone() {
        for bpm in TEMPOS {
            let found = detect(&synth::kick(bpm, 10.0, RATE)).unwrap();
            assert!((found.bpm - bpm).abs() <= 0.5, "{bpm}: got {}", found.bpm);
        }
    }

    #[test]
    fn finds_the_tempo_of_the_spec_loops() {
        let a_minor = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        for bpm in TEMPOS {
            let found = detect(&synth::test_loop(bpm, a_minor, 10.0, RATE)).unwrap();
            assert!((found.bpm - bpm).abs() <= 0.5, "{bpm}: got {}", found.bpm);
            assert!((0.0..=1.0).contains(&found.confidence));
        }
    }

    #[test]
    fn folds_slow_and_fast_tempos_into_the_spec_range() {
        let slow = detect(&synth::kick(60.0, 12.0, RATE)).unwrap();
        assert!((slow.bpm - 120.0).abs() <= 0.5, "60 BPM: got {}", slow.bpm);
        let fast = detect(&synth::kick(200.0, 10.0, RATE)).unwrap();
        assert!((fast.bpm - 100.0).abs() <= 0.5, "200 BPM: got {}", fast.bpm);
    }

    #[test]
    fn no_beat_means_no_tempo() {
        assert_eq!(detect(&[]), None);
        assert_eq!(detect(&vec![0.0; 10 * RATE as usize]), None);
        assert_eq!(detect(&synth::kick(124.0, 1.0, RATE)), None, "too short");
        let mut broken = synth::kick(124.0, 10.0, RATE);
        broken[100] = f32::NAN;
        assert_eq!(detect(&broken), None, "NaN sample");
        assert_eq!(
            OnsetTempo.detect_tempo(&synth::kick(124.0, 10.0, RATE), 0),
            None
        );
    }

    #[test]
    fn a_steady_tone_has_no_beat() {
        for hz in [30.0, 55.0, 220.0, 440.0] {
            assert_eq!(detect(&synth::sine(hz, -6.0, 10.0, RATE)), None, "{hz} Hz");
        }
        let kicks = detect(&synth::kick(124.0, 10.0, RATE)).unwrap();
        assert!(kicks.confidence > 0.8, "clean kicks: {}", kicks.confidence);
    }

    #[test]
    fn confidence_does_not_depend_on_clip_length() {
        let short = detect(&synth::kick(124.0, 4.0, RATE)).unwrap().confidence;
        let long = detect(&synth::kick(124.0, 30.0, RATE)).unwrap().confidence;
        assert!((short - long).abs() < 0.05, "4 s: {short}, 30 s: {long}");
    }

    /// A groove at `bpm`: kick on beats 1 and 3, snare-like noise on 2 and 4,
    /// and hi-hats on every eighth note at `hat_db`.
    fn groove(bpm: f32, hat_db: f32) -> Vec<f32> {
        let seconds = 20.0;
        let kick = synth::kick(bpm / 2.0, seconds, RATE);
        let snare = synth::hats(bpm / 2.0, 1, 0.5, -6.0, seconds, RATE);
        let hats = synth::hats(bpm, 2, 0.0, hat_db, seconds, RATE);
        synth::normalize(synth::mix(&[&kick, &snare, &hats]), -1.0)
    }

    #[test]
    fn slow_grooves_with_eighth_note_hats_keep_their_tempo() {
        for bpm in [75.0, 85.0, 88.0] {
            for hat_db in [-14.0, -8.0] {
                let found = detect(&groove(bpm, hat_db)).unwrap();
                assert!(
                    (found.bpm - bpm).abs() <= 0.5,
                    "{bpm} BPM, hats at {hat_db} dB: got {}",
                    found.bpm
                );
            }
        }
    }

    #[test]
    fn the_same_audio_always_gives_the_same_tempo() {
        let audio = synth::kick(128.0, 10.0, RATE);
        assert_eq!(detect(&audio), detect(&audio));
    }

    #[test]
    fn folding_keeps_in_range_values_and_halves_or_doubles_the_rest() {
        assert_eq!(fold_into_range(124.0), 124.0);
        assert_eq!(fold_into_range(248.0), 124.0);
        assert_eq!(fold_into_range(46.0), 92.0);
        assert_eq!(fold_into_range(180.0), 180.0);
        assert_eq!(fold_into_range(70.0), 70.0);
    }
}
