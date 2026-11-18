//! Track analysis (SPEC, Module 2): key and Camelot types, tempo and key
//! detection, loudness and the energy score. The waveform joins later (see
//! docs/ARCHITECTURE.md and docs/analysis.md).

pub mod energy;
pub mod key;
pub mod key_detect;
pub mod loudness;
#[cfg(feature = "stratum")]
pub mod stratum;
pub mod tempo;
