//! Reference-track matching (#27, SPEC Module 3): the long-term average
//! spectrum of a track and of a reference in 31 third-octave bands, and the
//! EQ that moves the track toward the reference. The curve is their
//! difference without the overall level (loudness targeting sets the level),
//! smoothed across neighbouring bands and clamped to +/-6 dB. It runs as 31
//! peaking filters, first in the preset chain (docs/ARCHITECTURE.md,
//! Mastering chain), scaled by the intensity like the preset's own EQ.

use std::array;
use std::ops::RangeInclusive;

use realfft::RealFftPlanner;

use super::eq::Coefficients;
use crate::CoreError;
use crate::audio::AudioBuffer;
use crate::progress::{CancellationToken, ProgressSink};

/// The ISO third-octave bands from 20 Hz to 20 kHz.
pub const BANDS: usize = 31;
/// The largest boost or cut of the curve, in dB (SPEC).
pub const MAX_GAIN_DB: f32 = 6.0;
/// Each band's filter is a third of an octave wide (bandwidth form, so the
/// width holds at every sample rate).
const BAND_OCTAVES: f64 = 1.0 / 3.0;
/// The bands whose average difference is the overall level taken off the
/// curve: 63 Hz to 10 kHz, where music has content.
const LEVEL_BANDS: RangeInclusive<usize> = 5..=27;
/// A band whose difference (without the level) is wider than this, in dB,
/// is a different arrangement there (an instrument one track has and the
/// other lacks), not a tonal balance an EQ can match: it is left alone.
const GAP_DB: f32 = 15.0;
/// Below this the curve is smoothed over an octave each way, not two
/// thirds: a bass line's few notes would otherwise show as peaks.
const WIDE_SMOOTHING_BELOW_HZ: f64 = 160.0;
/// Bands below this (20, 25 and 31.5 Hz) are only ever cut: a boost there
/// mostly adds rumble under the producer's own high pass and eats the
/// limiter's headroom.
const NO_BOOST_BELOW_HZ: f64 = 35.0;
/// A band this far under its spectrum's loudest band has no usable content
/// (above a lossy file's cutoff or the Nyquist frequency): the curve leaves
/// it alone instead of cutting it by the full 6 dB.
const NO_CONTENT_DB: f32 = 60.0;
/// The level of a band with no signal at all, in dB (the floor of the
/// measurement; real audio, even very quiet, is far above it).
const FLOOR_DB: f32 = -300.0;
/// Bands at or above this fraction of the sample rate get no filter: a
/// peaking filter that close to the Nyquist frequency is too warped.
const TOP_FRACTION: f64 = 0.45;
/// Bands with a filter: all but the 20 kHz band, which sits too close to
/// the Nyquist frequency at 44.1 and 48 kHz for one design to hold at both.
const FILTERED_BANDS: usize = BANDS - 1;
/// Rounds of the overlap correction in [`MatchCurve::new`].
const CORRECTION_ROUNDS: usize = 30;
/// The rate the filter gains are corrected at. With bandwidth-form filters
/// and no 20 kHz filter, the same gains land within 0.4 dB of the curve at
/// every rate from 44.1 to 192 kHz.
const DESIGN_RATE: u32 = 48_000;
/// How often the spectrum checks for a cancel, in FFT frames (a few seconds
/// of audio at most).
const CANCEL_EVERY: usize = 8;

/// The centre of band `band` in Hz (0 is 20 Hz, 17 is 1 kHz, 30 is
/// 20 kHz): the exact base-ten third-octave frequencies, `1000 * 10^(n/10)`.
#[must_use]
pub fn band_centre_hz(band: usize) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "a band index is under 31")]
    let n = band as f64 - 17.0;
    1000.0 * 10f64.powf(n / 10.0)
}

/// A long-term average spectrum: the level of each band, in dB. Only
/// differences mean something (between bands, and between two spectra).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spectrum {
    /// Band levels, from 20 Hz to 20 kHz.
    pub bands_db: [f32; BANDS],
}

/// The FFT size for `sample_rate`: bins at most 3 Hz wide, so even the
/// 20 Hz band (4.6 Hz wide) gets measured.
fn fft_size(sample_rate: u32) -> usize {
    (sample_rate as usize / 3).next_power_of_two().max(1024)
}

/// A Hann window of `size` points.
fn hann(size: usize) -> Vec<f32> {
    #[expect(clippy::cast_precision_loss, reason = "FFT sizes are exact in f64")]
    let step = std::f64::consts::TAU / size as f64;
    (0..size)
        .map(|i| {
            #[expect(clippy::cast_precision_loss, reason = "under the FFT size")]
            let phase = step * i as f64;
            #[expect(clippy::cast_possible_truncation, reason = "a window weight, 0 to 1")]
            let weight = (0.5 - 0.5 * phase.cos()) as f32;
            weight
        })
        .collect()
}

/// Measures the long-term average spectrum of `audio`: Hann-windowed FFTs
/// with half overlap over the whole track, power averaged over frames and
/// channels (so the sides of a stereo mix count, with no cancellation), then
/// summed in each band, a bin shared between two bands by how much of it
/// falls in each. Progress goes from 0 to 1.
///
/// # Errors
///
/// [`CoreError::Cancelled`] when `cancel` fires (checked every few seconds
/// of audio), or [`CoreError::InvalidAudio`] if the FFT fails.
pub fn long_term_spectrum(
    audio: &AudioBuffer,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<Spectrum, CoreError> {
    let rate = audio.sample_rate();
    let size = fft_size(rate);
    let hop = size / 2;
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
    let window = hann(size);
    let mut frame = fft.make_input_vec();
    let mut bins = fft.make_output_vec();
    let mut power = vec![0.0_f64; bins.len()];
    let frames = audio.frames();
    let starts = frames.div_ceil(hop).max(1);
    for (index, start) in (0..starts).map(|i| i * hop).enumerate() {
        if index % CANCEL_EVERY == 0 {
            cancel.check()?;
        }
        for channel in audio.channels() {
            let source = channel.get(start..).unwrap_or_default();
            for (i, (slot, &w)) in frame.iter_mut().zip(&window).enumerate() {
                *slot = source.get(i).map_or(0.0, |&sample| sample * w);
            }
            fft.process(&mut frame, &mut bins)
                .map_err(|_| CoreError::InvalidAudio("the spectrum could not be measured"))?;
            for (total, bin) in power.iter_mut().zip(&bins) {
                *total += f64::from(bin.norm_sqr());
            }
        }
        #[expect(clippy::cast_precision_loss, reason = "a progress fraction")]
        progress.progress((index + 1) as f32 / starts as f32);
    }

    // Averaged, so a spectrum does not depend on the track's length or
    // channel count.
    #[expect(clippy::cast_precision_loss, reason = "frame and channel counts")]
    let measured = (starts * audio.channels().len().max(1)) as f64;
    for total in &mut power {
        *total /= measured;
    }
    #[expect(clippy::cast_precision_loss, reason = "FFT sizes are exact in f64")]
    let width = f64::from(rate.max(1)) / size as f64;
    let edge = 10f64.powf(1.0 / 20.0);
    let bands_db = array::from_fn(|band| {
        let (low, high) = (band_centre_hz(band) / edge, band_centre_hz(band) * edge);
        // Bins from just below the band to just above it.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "positive bin indices, far under usize::MAX"
        )]
        let (first, last) = (
            (low / width - 0.5).floor().max(0.0) as usize,
            (high / width + 0.5).ceil() as usize,
        );
        let total: f64 = (first..=last.min(power.len().saturating_sub(1)))
            .map(|k| {
                #[expect(clippy::cast_precision_loss, reason = "a bin index")]
                let centre = k as f64 * width;
                let inside = (centre + width / 2.0).min(high) - (centre - width / 2.0).max(low);
                power.get(k).map_or(0.0, |&p| p * (inside.max(0.0) / width))
            })
            .sum();
        #[expect(clippy::cast_possible_truncation, reason = "a level in dB")]
        let level = (10.0 * total.max(10f64.powf(f64::from(FLOOR_DB) / 10.0)).log10()) as f32;
        level
    });
    Ok(Spectrum { bands_db })
}

/// Which bands of `spectrum` have usable content: above the floor, and
/// within [`NO_CONTENT_DB`] of its loudest band.
fn usable(spectrum: &Spectrum) -> [bool; BANDS] {
    let loudest = spectrum
        .bands_db
        .iter()
        .copied()
        .filter(|db| db.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    spectrum
        .bands_db
        .map(|db| db.is_finite() && db > FLOOR_DB + 10.0 && db > loudest - NO_CONTENT_DB)
}

/// The EQ curve, in dB per band, that moves a track with spectrum `track`
/// toward `reference`: the reference minus the track, less their average
/// difference from 63 Hz to 10 kHz (only the shape is matched; loudness
/// targeting sets the level; the median, so a few bands far apart do not
/// tilt it), smoothed with triangular weights two bands each way (an octave
/// each way below 160 Hz), so the curve follows the tonal balance rather
/// than which notes are played, and clamped to +/-6 dB. Left at 0: bands where either spectrum has no usable
/// content, bands more than 15 dB apart (a different arrangement, not a
/// balance), everything when nothing is comparable; below 35 Hz the curve
/// only cuts.
#[must_use]
pub fn match_target(track: &Spectrum, reference: &Spectrum) -> [f32; BANDS] {
    let (ours, theirs) = (usable(track), usable(reference));
    let valid: [bool; BANDS] = array::from_fn(|band| ours[band] && theirs[band]);
    let difference: [f32; BANDS] =
        array::from_fn(|band| reference.bands_db[band] - track.bands_db[band]);
    // The level: the median difference, so a few bands far apart (an
    // instrument only one track has) do not tilt the whole curve.
    let mut level_bands = [0.0_f32; BANDS];
    let mut count = 0;
    for band in LEVEL_BANDS.filter(|&band| valid[band]) {
        if let Some(slot) = level_bands.get_mut(count) {
            *slot = difference[band];
            count += 1;
        }
    }
    let Some(levels) = level_bands
        .get_mut(..count)
        .filter(|levels| !levels.is_empty())
    else {
        return [0.0; BANDS];
    };
    levels.sort_unstable_by(f32::total_cmp);
    let middle = levels.len() / 2;
    let level = if levels.len() % 2 == 0 {
        (levels[middle - 1] + levels[middle]) / 2.0
    } else {
        levels[middle]
    };
    let shape = |band: usize| difference[band] - level;
    let matched: [bool; BANDS] = array::from_fn(|band| valid[band] && shape(band).abs() <= GAP_DB);
    array::from_fn(|band| {
        if !matched[band] {
            return 0.0;
        }
        // A neighbour left out counts as this band's own value. Triangular
        // weights two bands each way, three (an octave) in the bass, where
        // a few notes would otherwise make peaks.
        let neighbour = |offset: isize| {
            band.checked_add_signed(offset)
                .filter(|&other| other < BANDS && matched[other])
                .map_or(shape(band), shape)
        };
        let radius: isize = if band_centre_hz(band) < WIDE_SMOOTHING_BELOW_HZ {
            3
        } else {
            2
        };
        let (total, weights) =
            (-radius..=radius).fold((0.0_f32, 0.0_f32), |(total, weights), offset| {
                #[expect(clippy::cast_precision_loss, reason = "a weight of at most 4")]
                let weight = (radius + 1 - offset.abs()) as f32;
                (total + weight * neighbour(offset), weights + weight)
            });
        let smoothed = total / weights;
        let gain = smoothed.clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
        if band_centre_hz(band) < NO_BOOST_BELOW_HZ {
            gain.min(0.0)
        } else {
            gain
        }
    })
}

/// The reference EQ: the gain of each band's peaking filter. Neighbouring
/// filters overlap, so the gains are corrected until the bank's response at
/// the band centres is the target curve. Plain values (`Copy`, no heap), so
/// it can travel to the audio thread inside a preset, where only the cheap
/// filters are built from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchCurve {
    gains_db: [f32; BANDS],
}

impl MatchCurve {
    /// No reference: every filter does nothing.
    pub const FLAT: Self = Self {
        gains_db: [0.0; BANDS],
    };

    /// The filters that reach `target_db` at the band centres (values that
    /// are not numbers count as 0, and every value is clamped to +/-6 dB).
    /// Corrects the gains a few dozen times, a millisecond or two of work:
    /// done once per reference, never on the audio thread.
    #[must_use]
    pub fn new(target_db: &[f32; BANDS]) -> Self {
        let target = target_db.map(|db| {
            if db.is_finite() {
                f64::from(db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB))
            } else {
                0.0
            }
        });
        let limit = f64::from(MAX_GAIN_DB) * 2.0;
        let mut gains: [f64; BANDS] = array::from_fn(|band| {
            if band < FILTERED_BANDS {
                target[band]
            } else {
                0.0
            }
        });
        for _ in 0..CORRECTION_ROUNDS {
            let filters = band_filters(DESIGN_RATE, |band| gains[band]);
            for (band, gain) in gains.iter_mut().enumerate().take(FILTERED_BANDS) {
                let centre = band_centre_hz(band);
                let response: f64 = filters
                    .iter()
                    .map(|filter| filter.response_db(DESIGN_RATE, centre))
                    .sum();
                *gain = (*gain + target[band] - response).clamp(-limit, limit);
            }
        }
        #[expect(clippy::cast_possible_truncation, reason = "gains within +/-12 dB")]
        let gains_db = gains.map(|gain| gain as f32);
        Self { gains_db }
    }

    /// The curve that moves `track` toward `reference` ([`match_target`]).
    #[must_use]
    pub fn between(track: &Spectrum, reference: &Spectrum) -> Self {
        Self::new(&match_target(track, reference))
    }

    /// Whether every filter does nothing.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.gains_db.iter().all(|&gain| gain == 0.0)
    }

    /// The gain of each band's filter, in dB.
    #[must_use]
    pub fn gains_db(&self) -> &[f32; BANDS] {
        &self.gains_db
    }

    /// The filters at `sample_rate`, their gains scaled by `intensity` (0 to
    /// 1, clamped; one that is not a number is 1): a filter that does
    /// nothing where the gain is 0 and for bands too close to the Nyquist
    /// frequency. Cheap and allocation-free (fine on the audio thread).
    #[must_use]
    pub fn coefficients(&self, sample_rate: u32, intensity: f32) -> [Coefficients; BANDS] {
        let k = if intensity.is_nan() {
            1.0
        } else {
            f64::from(intensity.clamp(0.0, 1.0))
        };
        band_filters(sample_rate, |band| {
            f64::from(self.gains_db.get(band).copied().unwrap_or(0.0)) * k
        })
    }
}

/// One peaking filter per band with the gain `gain(band)` in dB (one that
/// does nothing for a 0 gain or a band too close to the Nyquist frequency).
fn band_filters(sample_rate: u32, gain: impl Fn(usize) -> f64) -> [Coefficients; BANDS] {
    let top = f64::from(sample_rate) * TOP_FRACTION;
    array::from_fn(|band| {
        let (centre, gain_db) = (band_centre_hz(band), gain(band));
        if gain_db == 0.0 || centre >= top || band >= FILTERED_BANDS {
            Coefficients::identity()
        } else {
            Coefficients::peaking_octaves(sample_rate, centre, gain_db, BAND_OCTAVES)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mastering::eq::Biquad;
    use crate::progress::NoProgress;
    use crate::synth;

    const RATE: u32 = 48_000;

    /// White noise at about -20 dBFS (xorshift: the same every run).
    fn noise(seconds: f32) -> Vec<f32> {
        let mut state: u32 = 0x9E37_79B9;
        let len = (seconds * RATE as f32) as usize;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 - 0.5) * 0.2
            })
            .collect()
    }

    /// `samples` through `filters`, one after the other.
    fn filtered(samples: &[f32], filters: &[Coefficients]) -> Vec<f32> {
        let mut out = vec![samples.to_vec()];
        for &coefficients in filters {
            Biquad::new(coefficients, 1).process(&mut out);
        }
        out.remove(0)
    }

    fn spectrum(samples: Vec<f32>) -> Spectrum {
        let audio = AudioBuffer::new(RATE, vec![samples]).unwrap();
        long_term_spectrum(&audio, &NoProgress, &CancellationToken::new()).unwrap()
    }

    /// The bank's response at band `band`'s centre, in dB.
    fn response_at(filters: &[Coefficients; BANDS], rate: u32, band: usize) -> f64 {
        filters
            .iter()
            .map(|filter| filter.response_db(rate, band_centre_hz(band)))
            .sum()
    }

    fn mean(values: &[f32]) -> f32 {
        values.iter().sum::<f32>() / values.len() as f32
    }

    #[test]
    fn the_bands_are_the_iso_third_octaves() {
        assert!((band_centre_hz(0) - 19.95).abs() < 0.01);
        assert!((band_centre_hz(17) - 1000.0).abs() < 1e-9);
        assert!((band_centre_hz(30) - 19_952.6).abs() < 0.1);
        assert!((1..BANDS).all(|band| band_centre_hz(band) > band_centre_hz(band - 1)));
    }

    #[test]
    fn a_tone_lands_in_its_band() {
        let measured = spectrum(synth::sine(1000.0, -12.0, 4.0, RATE));
        let tone = measured.bands_db[17];
        for band in [14, 15, 19, 20] {
            assert!(tone - measured.bands_db[band] > 30.0, "band {band}");
        }
        // 20 Hz is measured too (bins under 3 Hz wide).
        let low = spectrum(synth::sine(20.0, -12.0, 4.0, RATE));
        assert!(low.bands_db[0] - low.bands_db[3] > 20.0);
    }

    #[test]
    fn the_spectrum_depends_on_neither_channel_count_nor_length() {
        let left = noise(6.0);
        let mono = spectrum(left.clone());
        let audio = AudioBuffer::new(RATE, vec![left.clone(), left.clone()]).unwrap();
        let stereo = long_term_spectrum(&audio, &NoProgress, &CancellationToken::new()).unwrap();
        let half = spectrum(left[..left.len() / 2].to_vec());
        for band in 0..BANDS {
            assert!(
                (stereo.bands_db[band] - mono.bands_db[band]).abs() < 0.01,
                "band {band}"
            );
            // From 100 Hz, where a band holds enough bins for 3 s of noise
            // to measure steadily.
            if band >= 10 {
                assert!(
                    (half.bands_db[band] - mono.bands_db[band]).abs() < 0.3,
                    "band {band}"
                );
            }
        }
        assert!(
            match_target(&mono, &stereo)
                .iter()
                .all(|&db| db.abs() < 0.01)
        );
    }

    #[test]
    fn measuring_reports_progress_and_can_be_cancelled() {
        let audio = AudioBuffer::new(RATE, vec![noise(3.0)]).unwrap();
        let last = std::sync::Mutex::new(0.0_f32);
        let sink = |fraction: f32| {
            let mut last = last.lock().unwrap();
            assert!(fraction >= *last);
            *last = fraction;
        };
        long_term_spectrum(&audio, &sink, &CancellationToken::new()).unwrap();
        assert_eq!(*last.lock().unwrap(), 1.0);
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            long_term_spectrum(&audio, &NoProgress, &cancel),
            Err(CoreError::Cancelled)
        ));
    }

    #[test]
    fn a_track_matched_to_itself_needs_no_eq() {
        let key = crate::analysis::key::Key {
            tonic: crate::analysis::key::PitchClass::A,
            mode: crate::analysis::key::Mode::Minor,
        };
        let track = spectrum(synth::test_loop(124.0, key, 6.0, RATE));
        assert_eq!(match_target(&track, &track), [0.0; BANDS]);
        assert!(MatchCurve::between(&track, &track).is_flat());
    }

    #[test]
    fn a_brighter_reference_asks_for_more_top_and_keeps_the_level() {
        let track = noise(4.0);
        let shelf = Coefficients::high_shelf(RATE, 4_000.0, 4.0, 1.0);
        let reference = filtered(&track, &[shelf]);
        let target = match_target(&spectrum(track), &spectrum(reference));
        let lows = mean(&target[5..=14]); // 63 Hz to 500 Hz
        let highs = mean(&target[26..=29]); // 8 kHz to 16 kHz
        assert!((highs - lows - 4.0).abs() < 0.5, "{target:?}");
        // Only the shape: below the shelf nothing moves (the level is the
        // median difference).
        assert!(target[5..=17].iter().all(|db| db.abs() < 0.3), "{target:?}");
    }

    #[test]
    fn the_curve_is_clamped_to_6_db() {
        let track = noise(4.0);
        for (gain, extreme) in [(12.0, MAX_GAIN_DB), (-12.0, -MAX_GAIN_DB)] {
            let shelf = Coefficients::high_shelf(RATE, 2_000.0, gain, 1.0);
            let target = match_target(
                &spectrum(track.clone()),
                &spectrum(filtered(&track, &[shelf])),
            );
            assert!(target.contains(&extreme), "{gain} dB: {target:?}");
            assert!(
                target.iter().all(|db| db.abs() <= MAX_GAIN_DB),
                "{target:?}"
            );
        }
    }

    #[test]
    fn bands_a_lossy_reference_cut_off_are_left_alone() {
        let track = noise(4.0);
        // A steep low pass at 14 kHz, as an MP3 cuts the top off.
        let low_pass = [Coefficients::low_pass(RATE, 14_000.0, 0.707); 8];
        let reference = filtered(&track, &low_pass);
        let target = match_target(&spectrum(track), &spectrum(reference));
        // Nothing left at 20 kHz: left alone rather than cut by 6 dB.
        assert_eq!(target[30], 0.0, "20 kHz: {target:?}");
        // Where the reference is only quieter, the difference stays.
        assert!(target[28] < -3.0, "12.5 kHz: {target:?}");
        // Below the cutoff, nothing to correct.
        assert!(target[5..=22].iter().all(|db| db.abs() < 1.0), "{target:?}");
    }

    #[test]
    fn nothing_comparable_gives_a_flat_curve() {
        let silent = spectrum(vec![0.0; RATE as usize]);
        let track = spectrum(noise(2.0));
        assert_eq!(match_target(&track, &silent), [0.0; BANDS]);
        assert_eq!(match_target(&silent, &track), [0.0; BANDS]);
    }

    #[test]
    fn the_filters_reach_the_curve_at_every_rate_despite_their_overlap() {
        let targets: [[f32; BANDS]; 3] = [
            // A smooth swing, a brighter top, the most the SPEC allows.
            array::from_fn(|band| 5.5 * (band as f32 / 3.0).sin()),
            array::from_fn(|band| if band >= 23 { 4.0 } else { 0.0 }),
            [MAX_GAIN_DB; BANDS],
        ];
        for target in targets {
            let curve = MatchCurve::new(&target);
            for rate in [44_100, 48_000, 88_200, 96_000, 192_000] {
                let filters = curve.coefficients(rate, 1.0);
                // Up to 16 kHz: the 20 kHz band has no filter.
                for (band, &want) in target.iter().enumerate().take(FILTERED_BANDS) {
                    let error = response_at(&filters, rate, band) - f64::from(want);
                    assert!(
                        error.abs() < 0.5,
                        "band {band} at {rate} Hz: off by {error:.2} dB"
                    );
                }
                assert_eq!(filters[BANDS - 1], Coefficients::identity());
            }
            assert_eq!(curve.gains_db()[BANDS - 1], 0.0);
        }
        // Uncorrected gains would overshoot: the correction matters.
        let target: [f32; BANDS] = array::from_fn(|band| 5.5 * (band as f32 / 3.0).sin());
        let naive = band_filters(RATE, |band| f64::from(target[band]));
        let worst = (0..FILTERED_BANDS)
            .map(|band| (response_at(&naive, RATE, band) - f64::from(target[band])).abs())
            .fold(0.0, f64::max);
        assert!(worst > 1.0, "naive gains are off by {worst:.2} dB at most");
    }

    #[test]
    fn intensity_scales_the_filters_and_flat_means_none() {
        let target = [3.0_f32; BANDS];
        let curve = MatchCurve::new(&target);
        assert!(
            curve
                .coefficients(RATE, 0.0)
                .iter()
                .all(|filter| *filter == Coefficients::identity())
        );
        let half = curve.coefficients(RATE, 0.5);
        assert!((response_at(&half, RATE, 17) - 1.5).abs() < 0.3);
        assert!(MatchCurve::FLAT.is_flat());
        assert!(
            MatchCurve::FLAT
                .coefficients(RATE, 1.0)
                .iter()
                .all(|filter| *filter == Coefficients::identity())
        );
        // Values that are not numbers count as 0.
        let mut broken = [0.0_f32; BANDS];
        broken[10] = f32::NAN;
        assert!(MatchCurve::new(&broken).is_flat());
    }

    /// A minor or D minor, for the same arrangement in two keys.
    fn key(tonic: crate::analysis::key::PitchClass) -> crate::analysis::key::Key {
        crate::analysis::key::Key {
            tonic,
            mode: crate::analysis::key::Mode::Minor,
        }
    }

    #[test]
    fn the_same_arrangement_in_another_key_asks_for_little() {
        use crate::analysis::key::PitchClass;
        // The worst case: a synthetic loop whose bass is one held sine.
        let a = spectrum(synth::test_loop(124.0, key(PitchClass::A), 8.0, RATE));
        let d = spectrum(synth::test_loop(124.0, key(PitchClass::D), 8.0, RATE));
        let target = match_target(&a, &d);
        assert!(target.iter().all(|db| db.abs() <= 3.5), "{target:?}");
        assert!(
            target[13..].iter().all(|db| db.abs() <= 2.5),
            "above 160 Hz: {target:?}"
        );
    }

    #[test]
    fn a_high_passed_track_gets_no_sub_boost() {
        let high_pass = [Coefficients::high_pass(RATE, 45.0, 0.707); 2];
        let target = match_target(
            &spectrum(filtered(&noise(4.0), &high_pass)),
            &spectrum(noise(4.0)),
        );
        assert!(
            target[..3].iter().all(|&db| db <= 0.0),
            "under 35 Hz: {target:?}"
        );
        // The cut a darker reference asks for there still applies (20 Hz,
        // 28 dB down, is a gap and left alone).
        let target = match_target(
            &spectrum(noise(4.0)),
            &spectrum(filtered(&noise(4.0), &high_pass)),
        );
        assert!(target[2] < -3.0, "{target:?}");
    }

    #[test]
    fn a_band_far_apart_is_a_different_arrangement_and_left_alone() {
        // The reference has a loud 1 kHz line the track does not.
        let track = noise(4.0);
        let line = synth::sine(1000.0, -6.0, 4.0, RATE);
        let reference: Vec<f32> = track.iter().zip(&line).map(|(n, s)| n + s).collect();
        let target = match_target(&spectrum(track), &spectrum(reference));
        assert_eq!(target[17], 0.0, "{target:?}");
        // The rest stays flat: the median keeps the level.
        assert!(target.iter().all(|db| db.abs() < 1.0), "{target:?}");
    }
}
