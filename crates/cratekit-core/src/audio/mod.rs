//! Audio in memory (SPEC, Module 1): decoding files to planar `f32`
//! ([`decode_file`]), the [`AudioBuffer`] that holds the result, and sample
//! rate conversion ([`resample`]), and reading tags without decoding
//! ([`probe_file`]). See docs/audio.md.

mod buffer;
mod decode;
mod probe;
mod resample;

pub use buffer::AudioBuffer;
pub use decode::decode_file;
pub use probe::{FileInfo, probe_file};
pub use resample::resample;
