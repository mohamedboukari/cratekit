//! Cratekit real-time playback engine.
//!
//! [`Engine`] owns the cpal output stream on its own thread and talks to the
//! audio callback ([`renderer::Renderer`]) only through lock-free queues and
//! atomics. Everything reachable from the callback follows the real-time
//! rules in `docs/ARCHITECTURE.md` (Threads): no allocation, locks, I/O,
//! logging or panics. See docs/playback.md.

mod engine;
pub mod renderer;

pub use engine::{
    Engine, EngineError, EngineOptions, OutputDevice, Status, default_device_id, is_fatal,
    output_device_ids, output_devices, prepare_track,
};
pub use renderer::{DeckId, Track};

/// Number of output channels the engine renders (stereo).
pub const OUTPUT_CHANNELS: usize = 2;
