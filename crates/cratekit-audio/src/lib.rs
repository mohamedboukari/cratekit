//! Cratekit real-time playback engine.
//!
//! Owns the cpal output stream and the audio thread. Everything reachable from
//! the audio callback follows the real-time rules in `docs/ARCHITECTURE.md`: no allocation, locks,
//! I/O, logging or panics. The engine, mixer, filter and meters arrive in the
//! Phase 1 playback task.

/// Number of output channels the engine renders (stereo).
pub const OUTPUT_CHANNELS: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_stereo() {
        assert_eq!(OUTPUT_CHANNELS, 2);
    }
}
