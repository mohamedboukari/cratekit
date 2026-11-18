//! Loudness (SPEC, Module 2): integrated loudness in LUFS and true peak in
//! dBTP, as defined by ITU-R BS.1770 and EBU R128.
//!
//! The measurement comes from the `ebur128` crate (a pure Rust port of the
//! reference libebur128). It sits behind [`measure_loudness`], so the rest of
//! Cratekit never depends on it directly and it can be swapped.
//!
//! - **Integrated loudness** is the K-weighted mean power of the whole track,
//!   measured in 400 ms blocks with two gates: blocks below -70 LUFS
//!   (silence) are dropped, then blocks more than 10 LU below the level of
//!   the remaining ones (quiet intros and breaks) are dropped too.
//! - **True peak** is the highest level of the signal between samples, found
//!   by oversampling 4 times (below 96 kHz). It can be above the highest
//!   sample: two samples at -3 dBFS can sit either side of a 0 dBTP peak.

use ebur128::{EbuR128, Mode};

use crate::CoreError;
use crate::progress::{CancellationToken, NoProgress, ProgressSink};

/// Integrated loudness and true peak of a whole track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated (gated) loudness in LUFS. EBU R128 broadcast is -23 LUFS;
    /// club and streaming masters are usually between -14 and -6.
    pub integrated_lufs: f32,
    /// The highest true peak across all channels, in dBTP (dB relative to
    /// full scale, between samples included).
    pub true_peak_dbtp: f32,
}

/// Measures the integrated loudness and true peak of `channels`, one slice
/// per channel (planar), all the same length, at `sample_rate`.
///
/// Channels follow the usual order: mono, stereo (left, right), or 5.1
/// (left, right, centre, LFE, left surround, right surround; the LFE is not
/// counted, as BS.1770 says). A mono track counts as one channel, so a mono
/// tone reads 3 LU lower than the same tone on both stereo channels.
///
/// Returns `None` when there is nothing to measure: no channels, channels of
/// different lengths, a sample rate the meter cannot use, samples that are
/// not finite numbers, audio shorter than one 400 ms gating block, or audio
/// that stays below the -70 LUFS silence gate.
#[must_use]
pub fn measure_loudness(channels: &[&[f32]], sample_rate: u32) -> Option<Loudness> {
    // A fresh token is never cancelled, so this cannot be an error.
    measure_loudness_with(
        channels,
        sample_rate,
        &NoProgress,
        &CancellationToken::new(),
    )
    .ok()
    .flatten()
}

/// [`measure_loudness`] that reports progress (0 to 1) and stops with
/// [`CoreError::Cancelled`] soon after `cancel` is cancelled. The meter is
/// fed `CHUNK_SECONDS` at a time, which gives the same result as one call.
///
/// # Errors
///
/// [`CoreError::Cancelled`] only; unusable audio is `Ok(None)`.
pub fn measure_loudness_with(
    channels: &[&[f32]],
    sample_rate: u32,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<Option<Loudness>, CoreError> {
    let Some(first) = channels.first() else {
        return Ok(None);
    };
    if channels.iter().any(|ch| ch.len() != first.len())
        || channels.iter().any(|ch| ch.iter().any(|s| !s.is_finite()))
    {
        return Ok(None);
    }
    let Ok(count) = u32::try_from(channels.len()) else {
        return Ok(None);
    };
    let Ok(mut meter) = EbuR128::new(count, sample_rate, Mode::I | Mode::TRUE_PEAK) else {
        return Ok(None);
    };
    let chunk = (CHUNK_SECONDS * sample_rate as usize).max(1);
    let length = first.len();
    let mut start = 0;
    while start < length {
        cancel.check()?;
        let end = (start + chunk).min(length);
        let part: Vec<&[f32]> = channels.iter().map(|ch| &ch[start..end]).collect();
        if meter.add_frames_planar_f32(&part).is_err() {
            return Ok(None);
        }
        start = end;
        progress.progress(start as f32 / length as f32);
    }
    Ok(finish(&meter, count))
}

/// How much audio the meter takes at a time.
const CHUNK_SECONDS: usize = 10;

/// The integrated loudness and the highest true peak, or `None` for silence.
fn finish(meter: &EbuR128, count: u32) -> Option<Loudness> {
    // Fully gated audio (silence) reads as negative infinity.
    let integrated = meter.loudness_global().ok()?;
    if !integrated.is_finite() {
        return None;
    }
    let mut peak = 0.0_f64;
    for ch in 0..count {
        peak = peak.max(meter.true_peak(ch).ok()?);
    }
    Some(Loudness {
        integrated_lufs: integrated as f32,
        true_peak_dbtp: (20.0 * peak.log10()) as f32,
    })
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

    use super::*;
    use crate::synth::{peak_db, sine};

    const RATE: u32 = 48_000;

    /// A 997 Hz test tone (the AES17 frequency, chosen so it never lines up
    /// with the sample rate), modelled on EBU Tech 3341 case 1, which uses
    /// 1 kHz.
    fn tone(level_db: f32, seconds: f32) -> Vec<f32> {
        sine(997.0, level_db, seconds, RATE)
    }

    fn stereo(samples: &[f32]) -> Option<Loudness> {
        measure_loudness(&[samples, samples], RATE)
    }

    fn assert_close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected} \u{b1} {tolerance}, got {actual}"
        );
    }

    #[test]
    fn a_minus_23_dbfs_tone_on_both_channels_reads_minus_23_lufs() {
        let loudness = stereo(&tone(-23.0, 10.0)).expect("a tone has a loudness");
        assert_close(loudness.integrated_lufs, -23.0, 0.1);
    }

    #[test]
    fn a_mono_tone_counts_as_one_channel() {
        let samples = tone(-23.0, 10.0);
        let loudness = measure_loudness(&[&samples], RATE).expect("a tone has a loudness");
        assert_close(loudness.integrated_lufs, -26.0, 0.1);
    }

    #[test]
    fn the_reference_level_holds_at_44_1_khz() {
        let samples = sine(997.0, -23.0, 10.0, 44_100);
        let loudness =
            measure_loudness(&[&samples, &samples], 44_100).expect("a tone has a loudness");
        assert_close(loudness.integrated_lufs, -23.0, 0.1);
    }

    #[test]
    fn silence_does_not_lower_the_loudness() {
        // The -70 LUFS absolute gate drops the silent half (without it, the
        // mean power would be 3 dB lower). The few 400 ms blocks that straddle
        // the end of the tone are partly loud, so they still count and pull the
        // result down a little: 20 s of tone keeps that well under 0.1 LU.
        let mut samples = tone(-20.0, 20.0);
        samples.extend(vec![0.0; 20 * RATE as usize]);
        let loudness = stereo(&samples).expect("half a tone has a loudness");
        assert_close(loudness.integrated_lufs, -20.0, 0.1);
    }

    #[test]
    fn a_much_quieter_part_does_not_lower_the_loudness() {
        // The quiet half is 30 dB down: above the silence gate, but more than
        // 10 LU below the loud half, so the relative gate drops it. Without
        // gating, the mean power would be about 3 dB lower.
        let mut samples = tone(-20.0, 10.0);
        samples.extend(tone(-50.0, 10.0));
        let loudness = stereo(&samples).expect("a tone has a loudness");
        assert_close(loudness.integrated_lufs, -20.0, 0.1);
    }

    #[test]
    fn a_louder_track_reads_louder_by_the_same_amount() {
        let quiet = stereo(&tone(-30.0, 10.0)).expect("a tone has a loudness");
        let loud = stereo(&tone(-12.0, 10.0)).expect("a tone has a loudness");
        assert_close(loud.integrated_lufs - quiet.integrated_lufs, 18.0, 0.05);
    }

    #[test]
    fn true_peak_finds_the_peak_between_samples() {
        // A sine at a quarter of the sample rate, shifted by 45 degrees: every
        // sample lands at +-0.707 (-3 dBFS), but the wave peaks at 1.0
        // (0 dBTP) halfway between two samples.
        let samples: Vec<f32> = (0..10 * RATE)
            .map(|n| (FRAC_PI_2 * f64::from(n) + FRAC_PI_4).sin() as f32)
            .collect();
        assert_close(peak_db(&samples), -3.01, 0.01);
        let loudness = stereo(&samples).expect("a tone has a loudness");
        assert_close(loudness.true_peak_dbtp, 0.0, 0.3);
    }

    #[test]
    fn true_peak_of_a_plain_tone_is_its_peak() {
        let loudness = stereo(&tone(-6.0, 10.0)).expect("a tone has a loudness");
        assert_close(loudness.true_peak_dbtp, -6.0, 0.1);
    }

    #[test]
    fn true_peak_is_the_loudest_channel() {
        let left = tone(-12.0, 10.0);
        let right = tone(-3.0, 10.0);
        let loudness = measure_loudness(&[&left, &right], RATE).expect("tones have a loudness");
        assert_close(loudness.true_peak_dbtp, -3.0, 0.1);
    }

    #[test]
    fn feeding_in_chunks_gives_the_same_result_and_reports_progress() {
        // 25 s: two full 10 s chunks and a partial one.
        let samples = tone(-18.0, 25.0);
        let whole = {
            let mut meter = EbuR128::new(2, RATE, Mode::I | Mode::TRUE_PEAK).expect("a meter");
            meter
                .add_frames_planar_f32(&[&samples, &samples])
                .expect("one call");
            finish(&meter, 2)
        };
        let seen = std::sync::Mutex::new(Vec::new());
        let sink = |fraction: f32| seen.lock().expect("lock").push(fraction);
        let chunked = measure_loudness_with(
            &[&samples, &samples],
            RATE,
            &sink,
            &CancellationToken::new(),
        )
        .expect("not cancelled");
        assert_eq!(chunked, whole);
        assert_eq!(seen.into_inner().expect("lock").len(), 3);

        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = measure_loudness_with(&[&samples], RATE, &NoProgress, &cancel);
        assert!(matches!(result, Err(CoreError::Cancelled)));
    }

    #[test]
    fn nothing_to_measure_gives_no_answer() {
        let loud = tone(-20.0, 10.0);
        let faint = tone(-90.0, 10.0);
        let silence = vec![0.0; 10 * RATE as usize];
        let mut broken = loud.clone();
        broken[RATE as usize] = f32::NAN;
        let mut infinite = loud.clone();
        infinite[RATE as usize] = f32::INFINITY;

        assert_eq!(measure_loudness(&[], RATE), None, "no channels");
        assert_eq!(stereo(&silence), None, "silence");
        assert_eq!(stereo(&[]), None, "empty channels");
        assert_eq!(stereo(&faint), None, "below the -70 LUFS gate");
        assert_eq!(stereo(&tone(-10.0, 0.3)), None, "shorter than one block");
        assert!(stereo(&tone(-10.0, 0.4)).is_some(), "one full block");
        assert_eq!(
            measure_loudness(&[&loud, &loud[1..]], RATE),
            None,
            "lengths differ"
        );
        assert_eq!(measure_loudness(&[&loud, &loud], 0), None, "no sample rate");
        assert_eq!(stereo(&broken), None, "NaN sample");
        assert_eq!(stereo(&infinite), None, "infinite sample");
    }
}
