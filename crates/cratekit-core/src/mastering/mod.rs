//! Mastering (SPEC Module 3): the blocks of the chain in
//! `docs/ARCHITECTURE.md`, Mastering chain. So far the DSP blocks (#23):
//! [`eq`] (biquad shelves, peaks and passes), [`compressor`] (the RMS glue
//! compressor) and [`limiter`] (the look-ahead true-peak limiter), and the
//! presets with their intensity (#24, [`presets`]), the whole chain with
//! loudness targeting (#25, [`master`]), reference-track matching (#27,
//! [`reference`]), and the soft clipper before the limiter (#144,
//! [`clipper`]).
//!
//! Every block processes planar `f32` in place and never allocates while
//! processing, so the live preview (#26) can run them in the audio callback.

pub mod clipper;
pub mod compressor;
pub mod eq;
pub mod limiter;
pub mod master;
pub mod presets;
pub mod reference;
