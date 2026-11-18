//! `AudioBuffer`: decoded audio in memory, planar `f32`.

use crate::CoreError;

/// Decoded audio: one `Vec<f32>` per channel (planar), all the same length,
/// at one sample rate. Full scale is an amplitude of 1.0.
///
/// The fields are private so every buffer keeps the rules checked by
/// [`AudioBuffer::new`]: at least one channel, all channels the same length,
/// and a sample rate above zero.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    sample_rate: u32,
    channels: Vec<Vec<f32>>,
}

impl AudioBuffer {
    /// A buffer from planar channels.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidAudio`] when there are no channels, the channels
    /// have different lengths, or the sample rate is zero.
    pub fn new(sample_rate: u32, channels: Vec<Vec<f32>>) -> Result<Self, CoreError> {
        if sample_rate == 0 {
            return Err(CoreError::InvalidAudio("the sample rate is zero"));
        }
        let Some(first) = channels.first() else {
            return Err(CoreError::InvalidAudio("there are no channels"));
        };
        if channels.iter().any(|ch| ch.len() != first.len()) {
            return Err(CoreError::InvalidAudio(
                "the channels have different lengths",
            ));
        }
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    /// Samples per second, per channel.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The channels, in file order (left then right for stereo).
    #[must_use]
    pub fn channels(&self) -> &[Vec<f32>] {
        &self.channels
    }

    /// Each channel as a borrowed slice, the shape
    /// [`measure_loudness`](crate::analysis::loudness::measure_loudness) takes.
    #[must_use]
    pub fn channel_slices(&self) -> Vec<&[f32]> {
        self.channels.iter().map(Vec::as_slice).collect()
    }

    /// Number of channels (1 for mono, 2 for stereo).
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Number of samples in each channel.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// Length in seconds.
    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// The average of all channels, sample by sample: the mono signal the
    /// tempo, key and energy detectors take.
    #[must_use]
    pub fn to_mono(&self) -> Vec<f32> {
        let count = self.channels.len() as f32;
        let mut mono = vec![0.0; self.frames()];
        for channel in &self.channels {
            for (sum, sample) in mono.iter_mut().zip(channel) {
                *sum += sample;
            }
        }
        for sample in &mut mono {
            *sample /= count;
        }
        mono
    }

    /// Gives back the channels, without copying them.
    #[must_use]
    pub fn into_channels(self) -> Vec<Vec<f32>> {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_checks_the_rules() {
        assert!(AudioBuffer::new(48_000, vec![vec![0.0; 4], vec![0.0; 4]]).is_ok());
        for (rate, channels) in [
            (0, vec![vec![0.0; 4]]),
            (48_000, vec![]),
            (48_000, vec![vec![0.0; 4], vec![0.0; 3]]),
        ] {
            assert!(matches!(
                AudioBuffer::new(rate, channels),
                Err(CoreError::InvalidAudio(_))
            ));
        }
    }

    #[test]
    fn reports_its_shape() {
        let buffer =
            AudioBuffer::new(4, vec![vec![0.5; 6], vec![-0.5; 6]]).expect("a valid buffer");
        assert_eq!(buffer.sample_rate(), 4);
        assert_eq!(buffer.channel_count(), 2);
        assert_eq!(buffer.frames(), 6);
        assert!((buffer.duration_seconds() - 1.5).abs() < f64::EPSILON);
        assert_eq!(buffer.channel_slices(), vec![&[0.5; 6][..], &[-0.5; 6][..]]);
    }

    #[test]
    fn mono_averages_the_channels() {
        let buffer = AudioBuffer::new(48_000, vec![vec![1.0, 0.5, 0.0], vec![0.0, 0.5, -1.0]])
            .expect("a valid buffer");
        assert_eq!(buffer.to_mono(), vec![0.5, 0.5, -0.5]);
        let mono = AudioBuffer::new(48_000, vec![vec![0.25, -0.25]]).expect("a valid buffer");
        assert_eq!(mono.to_mono(), vec![0.25, -0.25]);
        assert_eq!(mono.into_channels(), vec![vec![0.25, -0.25]]);
    }
}
