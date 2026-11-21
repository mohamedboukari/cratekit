//! The whole mastering chain (SPEC Module 3, #25): the reference match when
//! there is a reference (#27), the preset's EQ and compressor at its
//! intensity, then the makeup gain that lands on the
//! loudness target, found by measuring and correcting, then the true-peak
//! limiter, and a last check with the meter.
//!
//! The limiter makes loudness move less than the gain (under heavy limiting,
//! 0.15 to 0.4 dB per dB), so each correction divides the miss by the slope
//! measured over the last two passes. When the limiter cannot get there (a
//! very loud target on transient-rich material), the closest pass is kept and
//! [`Mastered::target_reached`] says so.
//!
//! Intensity never moves the target: the master lands on the loudness chosen
//! (`docs/ARCHITECTURE.md`, Mastering chain).

use crate::CoreError;
use crate::analysis::loudness::{Loudness, measure_loudness};
use crate::audio::AudioBuffer;
use crate::files::safe_file_name;
use crate::progress::{CancellationToken, ProgressSink};

use super::clipper::{ClipperSettings, clip};
use super::limiter::{LimiterSettings, limit};
use super::presets::{PresetChain, PresetName};
use super::reference::{MatchCurve, Spectrum, long_term_spectrum};

/// Close enough to the target, in LU (the SPEC accepts 0.5).
const TOLERANCE_LU: f32 = 0.3;
/// At most this many gain, limit and measure passes.
const MAX_PASSES: usize = 5;
/// The most makeup gain applied, in dB (a near-silent track is not blown up
/// without end).
const MAX_GAIN_DB: f32 = 40.0;
/// Loudness targets accepted, in LUFS: from very quiet to as loud as a
/// limiter at -1 dBTP can sensibly go.
const TARGET_RANGE: (f32, f32) = (-30.0, -5.0);
/// The loudness meter needs this much audio (one 400 ms gating block).
const MIN_SECONDS: f32 = 0.4;
/// The slope (LU of loudness per dB of gain) is kept in this range: the
/// limiter can flatten it but never makes it steeper than 1.
const SLOPE_RANGE: (f32, f32) = (0.05, 1.0);
/// A pass that moves the loudness less than this (LU) toward the target
/// means the limiter is at its limit: no point in more passes.
const STALL_LU: f32 = 0.05;
/// The share of the progress for the EQ and compressor pass (measuring the
/// track's spectrum for a reference included).
const COLOUR_SHARE: f32 = 0.3;
/// Of that share, measuring the track's spectrum when there is a reference.
const SPECTRUM_SHARE: f32 = 0.1;
/// Targets louder than this (in LUFS) may use the soft clipper (#144): a
/// club master. At this loudness and quieter the chain is limiter-only, as
/// before, so gentle masters never clip.
const CLIP_FROM_LUFS: f32 = -10.0;
/// The most of a master the clipper may touch (a share of its points): a
/// pass that clips more is never kept, and when louder would need more, the
/// target is out of reach.
const MAX_CLIP_SHARE: f32 = 0.05;
/// Headroom when the last check has to trim, in dB.
const TRIM_MARGIN_DB: f32 = 0.05;

/// What to master with: the UI's `MasterSettings` without the export
/// format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasterSettings {
    /// The preset.
    pub preset: PresetName,
    /// 0 to 1: how much the EQ and compressor colour the sound.
    pub intensity: f32,
    /// The loudness the master lands on, in LUFS (clamped to -30 to -5).
    pub target_lufs: f32,
    /// The reference track's long-term spectrum, to match (#27); `None`
    /// without a reference.
    pub reference: Option<Spectrum>,
}

impl MasterSettings {
    /// Whether this master may use the soft clipper: only for targets
    /// louder than -10 LUFS (the live preview asks the same question).
    #[must_use]
    pub fn clips(&self) -> bool {
        self.target() > CLIP_FROM_LUFS
    }

    /// The target actually aimed at: clamped to -30 to -5 LUFS, and the
    /// preset's own when it is not a number.
    #[must_use]
    pub fn target(&self) -> f32 {
        if self.target_lufs.is_finite() {
            self.target_lufs.clamp(TARGET_RANGE.0, TARGET_RANGE.1)
        } else {
            self.preset.preset().target_lufs
        }
    }
}

/// A mastered track and its measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct Mastered {
    /// The mastered audio, same rate and channels as the source.
    pub audio: AudioBuffer,
    /// Its integrated loudness, in LUFS (a mono track measured as it plays,
    /// on both speakers).
    pub lufs: f32,
    /// Its true peak, in dBTP (at or under -1.0).
    pub true_peak_dbtp: f32,
    /// The target it aimed at, in LUFS.
    pub target_lufs: f32,
    /// Whether it landed within 0.5 LU of the target (the SPEC acceptance);
    /// false when the limiter could not get there on this material.
    pub target_reached: bool,
    /// The share of the master the soft clipper touched, 0 to 1 (#144): 0
    /// for every master at -10 LUFS or quieter, at most 5 % on a louder one.
    pub clipped_share: f32,
}

/// Measures planar channels. A mono track is measured as dual mono (the same
/// channel on both sides), as it plays on a DJ system's two speakers, so it
/// lands as loud as a stereo track at the same target.
fn loudness(channels: &[Vec<f32>], sample_rate: u32) -> Result<Loudness, CoreError> {
    let mut slices: Vec<&[f32]> = channels.iter().map(Vec::as_slice).collect();
    if let [mono] = slices.as_slice() {
        slices = vec![*mono, *mono];
    }
    measure_loudness(&slices, sample_rate).ok_or(CoreError::InvalidAudio(
        "nothing to master: the track is silent",
    ))
}

/// Writes `source` times a gain in dB into `out` (same shape), reusing it.
fn apply_gain(source: &[Vec<f32>], gain_db: f32, out: &mut [Vec<f32>]) {
    let gain = 10_f32.powf(gain_db / 20.0);
    for (to, from) in out.iter_mut().zip(source) {
        for (sample, original) in to.iter_mut().zip(from) {
            *sample = original * gain;
        }
    }
}

/// Lowers `channels` when the meter reads their true peak over `ceiling`;
/// returns whether it did.
fn trim_to_ceiling(channels: &mut [Vec<f32>], true_peak_dbtp: f32, ceiling: f32) -> bool {
    if true_peak_dbtp <= ceiling {
        return false;
    }
    let gain = 10_f32.powf((ceiling - true_peak_dbtp - TRIM_MARGIN_DB) / 20.0);
    for sample in channels.iter_mut().flatten() {
        *sample *= gain;
    }
    true
}

/// The next gain to try: the miss divided by the slope the last two passes
/// showed (secant step), capped at +40 dB.
fn next_gain(gain_db: f32, miss: f32, slope: f32) -> f32 {
    (gain_db + miss / slope.clamp(SLOPE_RANGE.0, SLOPE_RANGE.1)).min(MAX_GAIN_DB)
}

/// Masters `audio`: the reference match when `settings.reference` is set
/// (the track's own spectrum is measured first, see
/// [`MatchCurve::between`]), the preset at its intensity, the loudness target within
/// 0.3 LU when the limiter allows it (the closest pass otherwise, with
/// `target_reached` false), and a true peak at or under -1.0 dBTP. Takes the
/// audio by value and works in place (three copies at most). Progress goes
/// from 0 to 1; cancellation is checked about every second of audio and
/// between passes.
///
/// # Errors
///
/// [`CoreError::InvalidAudio`] when the track is shorter than 0.4 s or
/// silent, or [`CoreError::Cancelled`].
pub fn master(
    audio: AudioBuffer,
    settings: &MasterSettings,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<Mastered, CoreError> {
    let rate = audio.sample_rate();
    let frames = audio.frames();
    if (frames as f32) < MIN_SECONDS * rate as f32 {
        return Err(CoreError::InvalidAudio("too short to master: under 0.4 s"));
    }
    let target = settings.target();

    // The reference match: the track's own spectrum against the reference's.
    let (curve, colour_start) = match &settings.reference {
        Some(reference) => {
            let measuring = |fraction: f32| progress.progress(SPECTRUM_SHARE * fraction);
            let track = long_term_spectrum(&audio, &measuring, cancel)?;
            (MatchCurve::between(&track, reference), SPECTRUM_SHARE)
        }
        None => (MatchCurve::FLAT, 0.0),
    };

    // The reference match, the preset's EQ and compressor, in place, a
    // second of audio at a time.
    let mut coloured = audio.into_channels();
    let preset = settings
        .preset
        .preset()
        .at(settings.intensity)
        .with_reference(curve);
    let mut chain = PresetChain::new(rate, coloured.len(), &preset);
    let block = rate.max(1) as usize;
    let mut start = 0;
    while start < frames {
        cancel.check()?;
        let end = (start + block).min(frames);
        let mut slices: Vec<&mut [f32]> = coloured.iter_mut().map(|c| &mut c[start..end]).collect();
        chain.process(&mut slices);
        start = end;
        progress
            .progress(colour_start + (COLOUR_SHARE - colour_start) * start as f32 / frames as f32);
    }

    // Makeup gain, limit, measure; correct by the miss over the measured
    // slope. `work` is reused for each pass and swapped into `best` when
    // closer, so there are never more than three copies of the audio.
    let before = loudness(&coloured, rate)?.integrated_lufs;
    let mut gain_db = (target - before).min(MAX_GAIN_DB);
    let mut work = coloured.clone();
    let mut best = coloured.clone();
    let mut best_measured: Option<Loudness> = None;
    let mut previous: Option<(f32, f32)> = None; // (gain, loudness) of the pass before
    let limiter = LimiterSettings::default();
    // The soft clipper takes the tips off peaks far over the ceiling, so the
    // limiter has less to catch (#144): for loud targets only.
    let clipper = settings
        .clips()
        .then(|| ClipperSettings::for_ceiling(limiter.ceiling_dbtp));
    let mut best_share = 0.0;
    for pass in 0..MAX_PASSES {
        cancel.check()?;
        apply_gain(&coloured, gain_db, &mut work);
        let share = clipper.map_or(0.0, |clipper| clip(&mut work, clipper));
        limit(&mut work, rate, limiter);
        let measured = loudness(&work, rate)?;
        let now = measured.integrated_lufs;
        let miss = target - now;
        progress
            .progress(COLOUR_SHARE + (1.0 - COLOUR_SHARE) * (pass + 1) as f32 / MAX_PASSES as f32);
        // A pass that clips more than the budget is never kept.
        let within_budget = share <= MAX_CLIP_SHARE;
        let closer = within_budget
            && best_measured.is_none_or(|b| miss.abs() < (target - b.integrated_lufs).abs());
        if closer {
            std::mem::swap(&mut best, &mut work);
            best_measured = Some(measured);
            best_share = share;
        }
        if within_budget && miss.abs() <= TOLERANCE_LU {
            break;
        }
        // Louder would clip more than the budget: as loud as it gets.
        if !within_budget && miss > 0.0 {
            break;
        }
        // The slope from the last two passes (1 to start: no limiting yet).
        let slope = previous
            .filter(|(gain, _)| (gain_db - gain).abs() > 1e-3)
            .map_or(1.0, |(gain, loud)| (now - loud) / (gain_db - gain));
        let stalled = previous.is_some_and(|(_, loud)| (now - loud).abs() < STALL_LU);
        let capped = gain_db >= MAX_GAIN_DB && miss > 0.0;
        if stalled || capped {
            break;
        }
        previous = Some((gain_db, now));
        gain_db = next_gain(gain_db, miss, slope);
    }
    // Every pass clipped more than the budget: the limiter alone, at the
    // last gain tried.
    if best_measured.is_none() {
        cancel.check()?;
        apply_gain(&coloured, gain_db.min(MAX_GAIN_DB), &mut best);
        limit(&mut best, rate, limiter);
        best_measured = Some(loudness(&best, rate)?);
        best_share = 0.0;
    }
    let Some(mut measured) = best_measured else {
        return Err(CoreError::Cancelled);
    };
    drop(work);
    drop(coloured);

    // The last check: the meter must read -1.0 dBTP or under.
    let ceiling = LimiterSettings::default().ceiling_dbtp;
    if trim_to_ceiling(&mut best, measured.true_peak_dbtp, ceiling) {
        measured = loudness(&best, rate)?;
    }
    progress.progress(1.0);
    Ok(Mastered {
        audio: AudioBuffer::new(rate, best)?,
        lufs: measured.integrated_lufs,
        true_peak_dbtp: measured.true_peak_dbtp,
        target_lufs: target,
        target_reached: (target - measured.integrated_lufs).abs() <= 0.5,
        clipped_share: best_share,
    })
}

/// The file name of a master: `<artist> - <title> (<preset> master).wav`
/// (the title or the artist alone when the other is empty). The track part
/// is made safe and shortened before the suffix, so two presets of the same
/// track never share a name.
#[must_use]
pub fn master_file_name(artist: &str, title: &str, preset: PresetName) -> String {
    let (artist, title) = (artist.trim(), title.trim());
    let label = match (artist.is_empty(), title.is_empty()) {
        (false, false) => format!("{artist} - {title}"),
        (true, _) => title.to_owned(),
        (false, true) => artist.to_owned(),
    };
    format!(
        "{} ({} master).wav",
        safe_file_name(&label, "Untitled"),
        preset.label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::audio::{WavFormat, decode_file, write_wav};
    use crate::synth;

    const RATE: u32 = 48_000;

    fn a_minor() -> Key {
        Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        }
    }

    /// The test loop in stereo (the right side a little quieter), peaking at
    /// `peak_db`.
    fn stereo_loop(peak_db: f32) -> AudioBuffer {
        let mix = synth::normalize(synth::test_loop(126.0, a_minor(), 4.0, RATE), peak_db);
        let other = mix.iter().map(|s| s * 0.9).collect();
        AudioBuffer::new(RATE, vec![mix, other]).unwrap()
    }

    /// Harder material: the loop with 16th-note hats, whose transients the
    /// limiter has to catch all the time.
    fn loop_with_hats(peak_db: f32, channels: usize) -> AudioBuffer {
        let mix = synth::normalize(
            synth::mix(&[
                &synth::test_loop(126.0, a_minor(), 4.0, RATE),
                &synth::hats(126.0, 4, 0.0, -6.0, 4.0, RATE),
            ]),
            peak_db,
        );
        AudioBuffer::new(RATE, vec![mix; channels]).unwrap()
    }

    fn settings(preset: PresetName, intensity: f32, target_lufs: f32) -> MasterSettings {
        MasterSettings {
            preset,
            intensity,
            target_lufs,
            reference: None,
        }
    }

    fn run(audio: &AudioBuffer, settings: &MasterSettings) -> Mastered {
        master(audio.clone(), settings, &|_| {}, &CancellationToken::new()).unwrap()
    }

    /// The SPEC acceptance, measured again on the output with the meter.
    fn assert_acceptance(mastered: &Mastered, target: f32, what: &str) {
        let measured = loudness(mastered.audio.channels(), RATE).unwrap();
        assert!(
            (measured.integrated_lufs - target).abs() <= 0.5,
            "{what}: {} LUFS for {target}",
            measured.integrated_lufs
        );
        assert!(
            measured.true_peak_dbtp <= -1.0,
            "{what}: {} dBTP",
            measured.true_peak_dbtp
        );
        assert!(
            (mastered.lufs - measured.integrated_lufs).abs() < 1e-3,
            "reports what it measured"
        );
        assert!((mastered.true_peak_dbtp - measured.true_peak_dbtp).abs() < 1e-3);
        assert!(mastered.target_reached, "{what}");
        assert_eq!(mastered.target_lufs, target);
    }

    #[test]
    fn lands_on_the_target_under_the_ceiling() {
        let source = stereo_loop(-1.0);
        for (preset, target) in [
            (PresetName::Club, -8.0),
            (PresetName::HipHop, -9.0),
            (PresetName::Chill, -14.0),
        ] {
            let mastered = run(&source, &settings(preset, 1.0, target));
            assert_acceptance(&mastered, target, &format!("{preset:?}"));
            assert_eq!(mastered.audio.channel_count(), 2);
            assert_eq!(mastered.audio.frames(), source.frames());
        }
    }

    #[test]
    fn reaches_the_target_on_transient_rich_material_by_following_the_limiter() {
        // Under heavy limiting the loudness moves far less than the gain:
        // a quiet track with hats still lands, in a few passes.
        let quiet = loop_with_hats(-30.0, 2);
        let passes = std::sync::atomic::AtomicUsize::new(0);
        let count = |fraction: f32| {
            if fraction > COLOUR_SHARE && fraction < 1.0 {
                passes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        };
        let mastered = master(
            quiet,
            &settings(PresetName::Pop, 1.0, -9.3),
            &count,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_acceptance(
            &mastered,
            -9.3,
            "hats from -30 dBFS, near the limiter's ceiling",
        );
        let passes = passes.into_inner();
        assert!(
            passes <= 3,
            "following the limiter's slope takes few passes: {passes}"
        );
        let fair = loop_with_hats(-1.0, 2);
        assert_acceptance(
            &run(&fair, &settings(PresetName::HipHop, 1.0, -11.0)),
            -11.0,
            "hats",
        );
    }

    #[test]
    fn club_reaches_minus_8_on_hat_heavy_material_with_the_clipper() {
        // #144: the limiter alone stops near -8.6 LUFS here.
        for channels in [2, 1] {
            let hats = loop_with_hats(-1.0, channels);
            let mastered = run(&hats, &settings(PresetName::Club, 1.0, -8.0));
            assert!(
                (mastered.lufs + 8.0).abs() <= TOLERANCE_LU,
                "{channels} ch: {} LUFS",
                mastered.lufs
            );
            assert_acceptance(&mastered, -8.0, "Club on hats");
            assert!(
                mastered.clipped_share > 0.0 && mastered.clipped_share <= MAX_CLIP_SHARE,
                "the clipper did part of it, within its budget: {}",
                mastered.clipped_share
            );
        }
    }

    /// A mix with far more peak than loudness, as unmastered tracks are: a
    /// kick at -1 dBFS over quiet chords and bass.
    fn peaky_mix() -> AudioBuffer {
        let kick = synth::normalize(synth::kick(124.0, 4.0, RATE), -1.0);
        let chords = synth::normalize(synth::chords(a_minor(), 124.0, 4.0, RATE), -18.0);
        let bass = synth::normalize(synth::bass(a_minor(), 124.0, 4.0, RATE), -18.0);
        let mix = synth::mix(&[&kick, &chords, &bass]);
        AudioBuffer::new(RATE, vec![mix.clone(), mix]).unwrap()
    }

    #[test]
    fn targets_at_minus_10_and_quieter_never_clip() {
        // Limiter-only, exactly as before #144, however peaky the mix.
        let (hats, peaky) = (loop_with_hats(-1.0, 2), peaky_mix());
        for (what, audio, preset) in [
            ("hats", &hats, PresetName::Chill),
            ("peaky mix", &peaky, PresetName::Chill),
            ("peaky mix", &peaky, PresetName::Club),
        ] {
            let mastered = run(audio, &settings(preset, 1.0, -12.0));
            assert_eq!(mastered.clipped_share, 0.0, "{what}, {preset:?}");
            assert_acceptance(&mastered, -12.0, what);
        }
        assert!(!settings(PresetName::Club, 1.0, -10.0).clips());
        assert!(settings(PresetName::Club, 1.0, -9.9).clips());
    }

    #[test]
    fn a_loud_target_that_needs_too_much_clipping_is_out_of_reach() {
        let hats = loop_with_hats(-1.0, 2);
        let mastered = run(&hats, &settings(PresetName::Club, 1.0, -6.0));
        assert!(!mastered.target_reached, "{} LUFS", mastered.lufs);
        assert!(
            mastered.clipped_share <= MAX_CLIP_SHARE,
            "{}",
            mastered.clipped_share
        );
        assert!(mastered.true_peak_dbtp <= -1.0);
        // A peaky mix at a club target stays within the budget too.
        let peaky = run(&peaky_mix(), &settings(PresetName::HipHop, 1.0, -9.0));
        assert!(
            peaky.clipped_share <= MAX_CLIP_SHARE,
            "{}",
            peaky.clipped_share
        );
        assert!(peaky.true_peak_dbtp <= -1.0);
    }

    #[test]
    fn says_so_when_the_limiter_cannot_reach_the_target() {
        let hats = loop_with_hats(-1.0, 2);
        let mastered = run(&hats, &settings(PresetName::Club, 1.0, -5.0));
        assert!(!mastered.target_reached);
        assert!(
            mastered.true_peak_dbtp <= -1.0,
            "{}",
            mastered.true_peak_dbtp
        );
        assert!(
            mastered.lufs > -11.0,
            "pushed as far as it goes: {}",
            mastered.lufs
        );
    }

    #[test]
    fn raises_a_quiet_track_and_lowers_a_hot_one() {
        let quiet = stereo_loop(-30.0);
        assert_acceptance(
            &run(&quiet, &settings(PresetName::Pop, 1.0, -9.0)),
            -9.0,
            "quiet",
        );
        let hot = stereo_loop(6.0);
        assert_acceptance(
            &run(&hot, &settings(PresetName::LoFi, 1.0, -14.0)),
            -14.0,
            "hot",
        );
    }

    #[test]
    fn a_mono_track_lands_as_loud_as_it_plays_on_two_speakers() {
        let mono = loop_with_hats(-1.0, 1);
        let mastered = run(&mono, &settings(PresetName::Chill, 1.0, -12.0));
        assert_eq!(mastered.audio.channel_count(), 1);
        assert_acceptance(&mastered, -12.0, "mono, measured as dual mono");
    }

    #[test]
    fn intensity_never_moves_the_loudness() {
        let source = stereo_loop(-1.0);
        for intensity in [0.0, 0.6] {
            let mastered = run(&source, &settings(PresetName::Club, intensity, -10.0));
            assert_acceptance(&mastered, -10.0, &format!("intensity {intensity}"));
        }
    }

    #[test]
    fn the_written_file_measures_what_was_reported() {
        let mastered = run(&stereo_loop(-1.0), &settings(PresetName::Club, 1.0, -9.0));
        let dir = std::env::temp_dir().join(format!("cratekit-master-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (format, bits) in [(WavFormat::Pcm16, 16), (WavFormat::Pcm24, 24)] {
            let path = dir.join(format!("master-{bits}.wav"));
            write_wav(&path, &mastered.audio, format).unwrap();
            let back = decode_file(&path).unwrap();
            let measured = loudness(back.channels(), RATE).unwrap();
            assert!(
                (measured.integrated_lufs - mastered.lufs).abs() < 0.01,
                "{format:?}"
            );
            assert!(
                measured.true_peak_dbtp <= -1.0,
                "{format:?}: {}",
                measured.true_peak_dbtp
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_last_check_trims_a_master_over_the_ceiling() {
        let mut channels = vec![vec![0.5_f32, -1.2, 0.9]];
        assert!(
            !trim_to_ceiling(&mut channels, -1.2, -1.0),
            "under: untouched"
        );
        assert_eq!(channels[0], [0.5, -1.2, 0.9]);
        assert!(trim_to_ceiling(&mut channels, -0.5, -1.0));
        let expected = 10_f32.powf(-0.55 / 20.0);
        assert!((channels[0][0] - 0.5 * expected).abs() < 1e-6);
    }

    #[test]
    fn the_gain_follows_the_measured_slope() {
        // 2 LU short with the loudness moving 0.25 per dB: 8 dB more.
        assert!((next_gain(10.0, 2.0, 0.25) - 18.0).abs() < 1e-6);
        // A slope near zero or above 1 is held in range.
        assert!((next_gain(10.0, 1.0, 0.0) - 30.0).abs() < 1e-6);
        assert!((next_gain(10.0, 1.0, 3.0) - 11.0).abs() < 1e-6);
        assert_eq!(next_gain(35.0, 10.0, 0.1), 40.0, "capped");
    }

    /// How far `audio`'s spectral shape is from `reference`'s: the mean
    /// absolute difference per band from 63 Hz to 10 kHz, the overall level
    /// taken off.
    fn shape_distance(audio: &AudioBuffer, reference: &Spectrum) -> f32 {
        let ours = long_term_spectrum(audio, &|_| {}, &CancellationToken::new()).unwrap();
        let diffs: Vec<f32> = (5..=27)
            .map(|band| reference.bands_db[band] - ours.bands_db[band])
            .collect();
        let level = diffs.iter().sum::<f32>() / diffs.len() as f32;
        diffs.iter().map(|d| (d - level).abs()).sum::<f32>() / diffs.len() as f32
    }

    #[test]
    fn a_reference_pulls_the_master_toward_its_sound() {
        let audio = loop_with_hats(-6.0, 2);
        // The same loop, brighter: the high shelf a darker mix would lack.
        let mut brighter = audio.clone().into_channels();
        let shelf = crate::mastering::eq::Coefficients::high_shelf(RATE, 5_000.0, 5.0, 1.0);
        crate::mastering::eq::Biquad::new(shelf, 2).process(&mut brighter);
        let reference = long_term_spectrum(
            &AudioBuffer::new(RATE, brighter).unwrap(),
            &|_| {},
            &CancellationToken::new(),
        )
        .unwrap();

        let plain = run(&audio, &settings(PresetName::Chill, 1.0, -12.0));
        let matched = run(
            &audio,
            &MasterSettings {
                reference: Some(reference),
                ..settings(PresetName::Chill, 1.0, -12.0)
            },
        );

        let (before, after) = (
            shape_distance(&plain.audio, &reference),
            shape_distance(&matched.audio, &reference),
        );
        assert!(after < before * 0.5, "{before:.2} dB then {after:.2} dB");
        assert_acceptance(&matched, -12.0, "matched to a reference");
    }

    #[test]
    fn reports_progress_and_can_be_cancelled_between_passes() {
        let source = stereo_loop(-1.0);
        let seen = std::sync::Mutex::new(Vec::new());
        let record = |fraction: f32| seen.lock().unwrap().push(fraction);
        master(
            source.clone(),
            &settings(PresetName::Club, 1.0, -9.0),
            &record,
            &CancellationToken::new(),
        )
        .unwrap();
        let seen = seen.into_inner().unwrap();
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "only forward");
        assert_eq!(seen.last(), Some(&1.0));

        // Cancelled once the first pass is done.
        let cancel = CancellationToken::new();
        let stop_after_colour = |fraction: f32| {
            if fraction > COLOUR_SHARE {
                cancel.cancel();
            }
        };
        let result = master(
            source.clone(),
            &settings(PresetName::Club, 1.0, -5.0),
            &stop_after_colour,
            &cancel,
        );
        assert!(matches!(result, Err(CoreError::Cancelled)));
        // Cancelled before the start.
        let early = CancellationToken::new();
        early.cancel();
        let result = master(
            source,
            &settings(PresetName::Club, 1.0, -9.0),
            &|_| {},
            &early,
        );
        assert!(matches!(result, Err(CoreError::Cancelled)));
    }

    #[test]
    fn silent_and_too_short_tracks_are_errors() {
        let silence = AudioBuffer::new(RATE, vec![vec![0.0; RATE as usize * 2]; 2]).unwrap();
        let result = master(
            silence,
            &settings(PresetName::Club, 1.0, -9.0),
            &|_| {},
            &CancellationToken::new(),
        );
        assert!(
            matches!(result, Err(CoreError::InvalidAudio(message)) if message.contains("silent"))
        );
        let blip = AudioBuffer::new(RATE, vec![synth::sine(440.0, -6.0, 0.3, RATE)]).unwrap();
        let result = master(
            blip,
            &settings(PresetName::Club, 1.0, -9.0),
            &|_| {},
            &CancellationToken::new(),
        );
        assert!(
            matches!(result, Err(CoreError::InvalidAudio(message)) if message.contains("too short"))
        );
    }

    #[test]
    fn the_target_is_clamped_and_falls_back_to_the_preset() {
        assert_eq!(settings(PresetName::Club, 1.0, 0.0).target(), -5.0);
        assert_eq!(settings(PresetName::Club, 1.0, -60.0).target(), -30.0);
        assert_eq!(settings(PresetName::Chill, 1.0, f32::NAN).target(), -12.0);
    }

    #[test]
    fn master_files_are_named_after_the_track_and_preset() {
        assert_eq!(
            master_file_name("DJ Test", "Night Drive", PresetName::Club),
            "DJ Test - Night Drive (Club master).wav"
        );
        assert_eq!(
            master_file_name("  ", "Solo", PresetName::LoFi),
            "Solo (Lo-fi master).wav"
        );
        assert_eq!(
            master_file_name("Artist", " ", PresetName::Chill),
            "Artist (Chill master).wav"
        );
        assert_eq!(
            master_file_name("", "", PresetName::Pop),
            "Untitled (Pop master).wav"
        );
        assert_eq!(
            master_file_name("A/B", "C:D", PresetName::Pop),
            "A_B - C_D (Pop master).wav"
        );
        // A very long title keeps the preset in the name: no two presets collide.
        let long = "x".repeat(300);
        let club = master_file_name("Artist", &long, PresetName::Club);
        let pop = master_file_name("Artist", &long, PresetName::Pop);
        assert_ne!(club, pop);
        assert!(club.ends_with(" (Club master).wav"));
        assert!(club.chars().count() < 160);
    }
}
