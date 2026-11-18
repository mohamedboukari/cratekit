//! Changing the sample rate of an [`AudioBuffer`] with `rubato`.

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

use crate::CoreError;
use crate::audio::AudioBuffer;

/// Frames the resampler works on at a time. Larger chunks are a little
/// faster; this is plenty for whole files.
const CHUNK_FRAMES: usize = 4096;

/// Converts `buffer` to `target_rate`, keeping its length in seconds and its
/// channels. A buffer already at that rate comes back unchanged, without a
/// copy.
///
/// Uses rubato's FFT resampler: a fixed ratio between the two rates, with an
/// anti-aliasing filter, for offline work (analysis, stems, mastering). Its
/// startup delay is trimmed, so the output lines up with the input sample for
/// sample.
///
/// # Errors
///
/// [`CoreError::InvalidAudio`] for a zero `target_rate`, and
/// [`CoreError::Resample`] if rubato rejects the rates or fails.
pub fn resample(buffer: AudioBuffer, target_rate: u32) -> Result<AudioBuffer, CoreError> {
    if target_rate == 0 {
        return Err(CoreError::InvalidAudio("the target sample rate is zero"));
    }
    if buffer.sample_rate() == target_rate || buffer.frames() == 0 {
        return AudioBuffer::new(target_rate, buffer.into_channels());
    }
    let channel_count = buffer.channel_count();
    let frames = buffer.frames();
    let failed = |err: &dyn std::fmt::Display| CoreError::Resample(err.to_string());

    let mut resampler = Fft::<f32>::new(
        buffer.sample_rate() as usize,
        target_rate as usize,
        CHUNK_FRAMES,
        channel_count,
        FixedSync::Input,
    )
    .map_err(|err| failed(&err))?;
    let input = SequentialSliceOfVecs::new(buffer.channels(), channel_count, frames)
        .map_err(|err| failed(&err))?;
    let capacity = resampler.process_all_needed_output_len(frames);
    let mut output = vec![vec![0.0_f32; capacity]; channel_count];
    let mut output_adapter = SequentialSliceOfVecs::new_mut(&mut output, channel_count, capacity)
        .map_err(|err| failed(&err))?;
    let (_, produced) = resampler
        .process_all_into_buffer(&input, &mut output_adapter, frames, None)
        .map_err(|err| failed(&err))?;
    // The exact length in whole frames, from integers: rubato's own count
    // rounds a floating-point ratio up and can be one frame long.
    let exact = (frames as u64 * u64::from(target_rate)).div_ceil(u64::from(buffer.sample_rate()));
    let length = usize::try_from(exact).map_or(produced, |exact| exact.min(produced));
    for channel in &mut output {
        channel.truncate(length);
    }
    AudioBuffer::new(target_rate, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{peak_db, sine, tone_power};

    fn tone(hz: f32, level_db: f32, seconds: f32, rate: u32) -> AudioBuffer {
        let samples = sine(hz, level_db, seconds, rate);
        AudioBuffer::new(rate, vec![samples.clone(), samples]).expect("a valid buffer")
    }

    #[test]
    fn keeps_the_length_pitch_and_level() {
        for (from, to) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 44_100),
            (22_050, 48_000),
        ] {
            let output = resample(tone(1000.0, -6.0, 2.0, from), to).expect("resamples");
            assert_eq!(output.sample_rate(), to);
            assert_eq!(output.channel_count(), 2);
            assert_eq!(output.frames(), 2 * to as usize, "{from} -> {to}");
            let left = &output.channels()[0];
            // The tone is still 1 kHz: far stronger there than a semitone away.
            let middle = &left[to as usize / 2..to as usize * 3 / 2];
            assert!(tone_power(middle, 1000.0, to) > 100.0 * tone_power(middle, 1059.5, to));
            assert!(
                (peak_db(middle) - -6.0).abs() < 0.1,
                "{from} -> {to}: {}",
                peak_db(middle)
            );
        }
    }

    #[test]
    fn the_length_is_exact_for_a_long_track() {
        // A minute at 44.1 kHz is exactly a minute at 48 kHz.
        let input = AudioBuffer::new(44_100, vec![vec![0.1; 60 * 44_100]]).expect("a valid buffer");
        let output = resample(input, 48_000).expect("resamples");
        assert_eq!(output.frames(), 60 * 48_000);
    }

    #[test]
    fn lines_up_with_the_input() {
        // A click at 1.0 s stays at 1.0 s: the resampler's delay is trimmed.
        let mut samples = vec![0.0_f32; 2 * 44_100];
        samples[44_100] = 1.0;
        let input = AudioBuffer::new(44_100, vec![samples]).expect("a valid buffer");
        let output = resample(input, 48_000).expect("resamples");
        let loudest = output.channels()[0]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(n, _)| n);
        assert!(
            loudest.is_some_and(|n| n.abs_diff(48_000) <= 1),
            "{loudest:?}"
        );
    }

    #[test]
    fn removes_what_the_new_rate_cannot_hold() {
        // 20 kHz fits at 48 kHz but not at 32 kHz (limit 16 kHz): filtered out,
        // not folded down to a false 12 kHz tone.
        let output = resample(tone(20_000.0, -6.0, 1.0, 48_000), 32_000).expect("resamples");
        assert!(peak_db(&output.channels()[0][4_000..28_000]) < -60.0);
    }

    #[test]
    fn the_same_rate_or_no_audio_comes_back_unchanged() {
        let input = tone(440.0, -6.0, 0.1, 48_000);
        assert_eq!(resample(input.clone(), 48_000).expect("no change"), input);
        let empty = AudioBuffer::new(44_100, vec![Vec::new()]).expect("a valid buffer");
        let output = resample(empty, 48_000).expect("nothing to do");
        assert_eq!((output.sample_rate(), output.frames()), (48_000, 0));
        assert!(matches!(
            resample(tone(440.0, -6.0, 0.1, 48_000), 0),
            Err(CoreError::InvalidAudio(_))
        ));
    }
}
