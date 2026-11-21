//! Stem separation (SPEC, Module 4): [`runtime`] loads ONNX Runtime at run
//! time and says why stems cannot run when it cannot, [`models`] is the
//! opt-in model download (the only network call in the app), [`dsp`] is the
//! signal processing around the model, and [`htdemucs::HtDemucs`] splits a
//! track behind the [`StemSeparator`] trait. See docs/stems.md and
//! docs/ARCHITECTURE.md, Stems notes.

pub mod dsp;
pub mod htdemucs;
pub mod models;
pub mod runtime;

use crate::CoreError;
use crate::audio::AudioBuffer;
use crate::progress::{CancellationToken, ProgressSink};

/// The four stems of a track, at 44.1 kHz stereo. They sum back to the
/// original.
#[derive(Debug, Clone, PartialEq)]
pub struct Stems {
    /// Drums.
    pub drums: AudioBuffer,
    /// Bass.
    pub bass: AudioBuffer,
    /// Everything that is not drums, bass or vocals (instruments, pads,
    /// effects).
    pub other: AudioBuffer,
    /// Vocals.
    pub vocals: AudioBuffer,
}

impl Stems {
    /// The 2-stem mode's instrumental: drums, bass and other summed, which
    /// is the mix without the vocals.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidAudio`] if the stems do not have the same shape
    /// (they always do when they come from a separator).
    pub fn instrumental(&self) -> Result<AudioBuffer, CoreError> {
        let channels = self
            .drums
            .channels()
            .iter()
            .zip(self.bass.channels())
            .zip(self.other.channels())
            .map(|((drums, bass), other)| {
                drums
                    .iter()
                    .zip(bass)
                    .zip(other)
                    .map(|((d, b), o)| d + b + o)
                    .collect()
            })
            .collect();
        AudioBuffer::new(self.drums.sample_rate(), channels)
    }
}

/// Splits a track into stems. The app depends on this trait, not on a model
/// or a runtime, so either can be swapped.
pub trait StemSeparator {
    /// Splits `audio` (any sample rate, mono or stereo) into the four stems.
    ///
    /// # Errors
    ///
    /// [`CoreError::Stems`] when the model fails, [`CoreError::Cancelled`],
    /// or a resampling error.
    fn separate(
        &mut self,
        audio: AudioBuffer,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
    ) -> Result<Stems, CoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(values: [f32; 3]) -> AudioBuffer {
        AudioBuffer::new(44_100, vec![values.to_vec(), values.map(|v| -v).to_vec()])
            .expect("buffer")
    }

    #[test]
    fn the_instrumental_is_everything_but_the_vocals() {
        let stems = Stems {
            drums: buffer([0.1, 0.2, 0.3]),
            bass: buffer([0.01, 0.02, 0.03]),
            other: buffer([0.001, 0.002, 0.003]),
            vocals: buffer([0.5, 0.5, 0.5]),
        };
        let instrumental = stems.instrumental().expect("instrumental");
        assert_eq!(instrumental.sample_rate(), 44_100);
        assert_eq!(instrumental.channel_count(), 2);
        for (got, want) in instrumental.channels()[0]
            .iter()
            .zip([0.111_f32, 0.222, 0.333])
        {
            assert!((got - want).abs() < 1e-6, "{got} {want}");
        }
        for (got, want) in instrumental.channels()[1]
            .iter()
            .zip([-0.111_f32, -0.222, -0.333])
        {
            assert!((got - want).abs() < 1e-6, "{got} {want}");
        }
    }
}
