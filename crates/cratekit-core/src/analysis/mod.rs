//! Track analysis (SPEC, Module 2): key and Camelot types, tempo and key
//! detection, loudness and the energy score. The waveform joins later (see
//! docs/ARCHITECTURE.md and docs/analysis.md).

/// Version of the analysis code. Bump it when a detector changes, so stored
/// results from older code count as needing analysis again.
pub const ANALYZER_VERSION: i64 = 1;

pub mod energy;
pub mod key;
pub mod key_detect;
pub mod loudness;
#[cfg(feature = "stratum")]
pub mod stratum;
pub mod tempo;
pub mod track;
