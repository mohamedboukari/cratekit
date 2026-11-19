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
