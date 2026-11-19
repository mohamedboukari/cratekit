//! Track analysis (SPEC, Module 2): key and Camelot types, tempo and key
//! detection, loudness and the energy score, the whole-track analysis and the
//! waveform columns (see docs/ARCHITECTURE.md and docs/analysis.md).

/// Version of the analysis code. Bump it when a detector changes, so stored
/// results from older code count as needing analysis again.
/// 2: adds the waveform (#13).
pub const ANALYZER_VERSION: i64 = 2;

pub mod energy;
pub mod key;
pub mod key_detect;
pub mod loudness;
#[cfg(feature = "stratum")]
pub mod stratum;
pub mod tempo;
pub mod track;
pub mod waveform;
