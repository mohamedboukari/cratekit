//! Mastering (SPEC Module 3): the blocks of the chain in
//! `docs/ARCHITECTURE.md`, Mastering chain. So far the DSP blocks (#23):
//! [`eq`] (biquad shelves, peaks and passes), [`compressor`] (the RMS glue
//! compressor) and [`limiter`] (the look-ahead true-peak limiter), and the
//! presets with their intensity (#24, [`presets`]). Loudness targeting and
//! export (#25) build on them.
//!
//! Every block processes planar `f32` in place and never allocates while
//! processing, so the live preview (#26) can run them in the audio callback.

pub mod compressor;
pub mod eq;
pub mod limiter;
pub mod presets;
