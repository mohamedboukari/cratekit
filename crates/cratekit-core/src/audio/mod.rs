//! Audio in memory (SPEC, Module 1): decoding files to planar `f32`
//! ([`decode_file`]), the [`AudioBuffer`] that holds the result, and sample
//! rate conversion ([`resample`]), reading tags without decoding
//! ([`probe_file`]), and writing WAV files ([`write_wav`]). See docs/audio.md.

mod buffer;
mod decode;
mod probe;
mod resample;
mod wav;

pub use buffer::AudioBuffer;
pub use decode::{decode_file, decode_file_with};
pub use probe::{FileInfo, probe_file};
pub use resample::resample;
pub(crate) use wav::with_suffix;
pub use wav::{WavFormat, write_wav};
