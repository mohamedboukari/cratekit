//! The real-time part: what runs inside the audio callback.
//!
//! [`Renderer::render`] fills the output buffer. It never allocates, frees,
//! locks, blocks, does I/O or panics (see the realtime rules in
//! docs/ARCHITECTURE.md, Threads): commands arrive over a lock-free queue,
//! replaced tracks leave over another one to be freed elsewhere, and position
//! and meters go out through atomics.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer};

use crate::filter::DjFilter;

/// Replaced tracks kept in the renderer when the garbage queue is full.
const RETIRED_SLOTS: usize = 4;

/// Fades for play, pause, seek and load last this long, so nothing clicks.
pub const FADE_SECONDS: f32 = 0.005;

/// A track ready to play: interleaved stereo `f32` at the engine's rate,
/// prepared off the audio thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    samples: Vec<f32>,
}

impl Track {
    /// A track from its left and right channels (the shorter length wins).
    #[must_use]
    pub fn from_stereo(left: &[f32], right: &[f32]) -> Self {
        let samples = left.iter().zip(right).flat_map(|(&l, &r)| [l, r]).collect();
        Self { samples }
    }

    /// The samples, interleaved left and right.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Number of stereo frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    /// The frame at `index`, or silence past the end.
    fn frame(&self, index: usize) -> (f32, f32) {
        let start = index.saturating_mul(2);
        match self.samples.get(start..start.saturating_add(2)) {
            Some([left, right]) => (*left, *right),
            _ => (0.0, 0.0),
        }
    }
}

/// Which deck a command or a published state belongs to. v0.1 has one deck;
/// Performance mode (v0.2, SPEC Module 8) adds a second, so the engine
/// carries the id from the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeckId {
    /// The only deck in v0.1.
    A,
}

/// What the engine asks the audio thread to do. Small and heap-free except
/// for `Load`, whose track is allocated before it is sent.
#[derive(Debug)]
pub enum Command {
    /// Replace the track (stops playback, position 0).
    Load(Box<Track>),
    /// Start or resume.
    Play,
    /// Pause (after a short fade).
    Pause,
    /// Jump to a frame (with a fade out and in).
    Seek(u64),
    /// Turn the DJ filter knob: -1 full low-pass, 0 off, 1 full high-pass
    /// (smoothed, so it never clicks).
    SetFilter(f32),
}

/// What the audio thread publishes, read by other threads.
#[derive(Debug, Default)]
pub struct Shared {
    /// Playback position in frames.
    pub position: AtomicU64,
    /// Whether the track is playing (the user's intent, true until paused or
    /// the end).
    pub playing: AtomicBool,
    /// How many times a track played to its end (a counter, so a reader
    /// notices every end).
    pub ends: AtomicU64,
    /// Highest absolute sample since a reader last took it (`f32` bits).
    pub peak: AtomicU32,
    /// RMS of the last rendered block (`f32` bits).
    pub rms: AtomicU32,
    /// Replaced tracks that could not be handed to the cleanup thread yet.
    pub garbage_waits: AtomicU64,
    /// Replaced tracks leaked because every slot was full (never freed on
    /// the audio thread). Should stay 0.
    pub leaked_tracks: AtomicU64,
}

/// The audio callback's own state. It owns everything it touches.
pub struct Renderer {
    commands: Consumer<(DeckId, Command)>,
    garbage: Producer<Box<Track>>,
    shared: Arc<Shared>,
    /// Output channels of the device (the track is stereo).
    channels: usize,
    /// Gain change per frame while fading.
    fade_step: f32,
    track: Option<Box<Track>>,
    /// A track waiting to replace the current one once the fade out ends.
    incoming: Option<Box<Track>>,
    /// Replaced tracks the garbage queue had no room for yet.
    retired: [Option<Box<Track>>; RETIRED_SLOTS],
    position: usize,
    playing: bool,
    gain: f32,
    target: f32,
    pending_seek: Option<usize>,
    filter: DjFilter,
}

impl Renderer {
    /// A renderer for a device with `channels` outputs at `sample_rate`,
    /// its DJ filter already at `filter` (-1 to 1).
    #[must_use]
    pub fn new(
        commands: Consumer<(DeckId, Command)>,
        garbage: Producer<Box<Track>>,
        shared: Arc<Shared>,
        channels: usize,
        sample_rate: u32,
        filter: f32,
    ) -> Self {
        let mut dj_filter = DjFilter::new(sample_rate);
        dj_filter.jump(filter);
        let fade_frames = (FADE_SECONDS * sample_rate as f32).max(1.0);
        Self {
            commands,
            garbage,
            shared,
            channels: channels.max(1),
            fade_step: 1.0 / fade_frames,
            track: None,
            incoming: None,
            retired: Default::default(),
            position: 0,
            playing: false,
            gain: 0.0,
            target: 0.0,
            pending_seek: None,
            filter: dj_filter,
        }
    }

    /// Fills `out` (interleaved, any length). Never allocates, frees, locks,
    /// blocks or panics.
    pub fn render(&mut self, out: &mut [f32]) {
        self.hand_back_retired();
        while let Ok((deck, command)) = self.commands.pop() {
            match deck {
                DeckId::A => self.apply(command),
            }
        }

        let (mut peak, mut squares) = (0.0_f32, 0.0_f32);
        let mut frames = 0_u32;
        let channels = self.channels;
        let mut blocks = out.chunks_exact_mut(channels);
        for frame in &mut blocks {
            let (left, right) = self.next_frame();
            let (left, right) = self.filter.process(left, right);
            match frame {
                [only] => *only = 0.5 * (left + right),
                [first, second, rest @ ..] => {
                    *first = left;
                    *second = right;
                    rest.fill(0.0);
                }
                [] => {}
            }
            peak = peak.max(left.abs()).max(right.abs());
            squares += 0.5 * (left * left + right * right);
            frames += 1;
        }
        // A partial frame at the end (should not happen) stays silent.
        blocks.into_remainder().fill(0.0);

        self.shared
            .position
            .store(self.position as u64, Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
        // Positive floats order like their bits, so fetch_max keeps the peak.
        self.shared
            .peak
            .fetch_max(peak.to_bits(), Ordering::Relaxed);
        let rms = if frames > 0 {
            (squares / frames as f32).sqrt()
        } else {
            0.0
        };
        self.shared.rms.store(rms.to_bits(), Ordering::Relaxed);
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Load(track) => {
                // A track that arrives during another fade replaces the
                // waiting one; the waiting one is handed back.
                if let Some(waiting) = self.incoming.replace(track) {
                    self.retire(waiting);
                }
                self.playing = false;
                self.pending_seek = None;
                self.target = 0.0;
            }
            Command::Play => {
                if self.track.is_some() || self.incoming.is_some() {
                    self.playing = true;
                    if self.pending_seek.is_none() && self.incoming.is_none() {
                        self.target = 1.0;
                    }
                }
            }
            Command::Pause => {
                self.playing = false;
                self.target = 0.0;
            }
            Command::Seek(frame) => {
                self.pending_seek = Some(usize::try_from(frame).unwrap_or(usize::MAX));
                self.target = 0.0;
            }
            Command::SetFilter(value) => self.filter.set(value),
        }
    }

    /// The next output frame, moving the fade and the position.
    fn next_frame(&mut self) -> (f32, f32) {
        if self.gain <= 0.0 && self.target <= 0.0 {
            self.gain = 0.0;
            self.settle();
        }
        if self.gain < self.target {
            self.gain = (self.gain + self.fade_step).min(self.target);
        } else if self.gain > self.target {
            self.gain = (self.gain - self.fade_step).max(self.target);
        }
        // The position moves while playing and through a fade out.
        let moving = self.playing || self.gain > 0.0;
        let Some(track) = self.track.as_deref() else {
            return (0.0, 0.0);
        };
        if !moving {
            return (0.0, 0.0);
        }
        if self.position >= track.frames() {
            // A seek or load waiting for its fade out is not an end: the
            // gain keeps falling and `settle` then applies it.
            if self.playing && self.pending_seek.is_none() && self.incoming.is_none() {
                self.playing = false;
                self.target = 0.0;
                self.gain = 0.0;
                self.shared.ends.fetch_add(1, Ordering::Relaxed);
            }
            return (0.0, 0.0);
        }
        let (left, right) = track.frame(self.position);
        self.position += 1;
        (left * self.gain, right * self.gain)
    }

    /// At silence: swap in a waiting track, apply a waiting seek, then fade
    /// back in if playing.
    fn settle(&mut self) {
        if self.incoming.is_some() {
            if let Some(old) = std::mem::replace(&mut self.track, self.incoming.take()) {
                self.retire(old);
            }
            self.position = 0;
        }
        if let Some(frame) = self.pending_seek.take() {
            let end = self.track.as_deref().map_or(0, Track::frames);
            self.position = frame.min(end);
        }
        if self.playing && self.incoming.is_none() {
            self.target = 1.0;
        }
    }

    /// Sends a replaced track to the cleanup thread to be freed there. When
    /// the queue is full it is kept in a slot and sent later; it must never
    /// be freed here.
    fn retire(&mut self, track: Box<Track>) {
        let Err(rtrb::PushError::Full(track)) = self.garbage.push(track) else {
            return;
        };
        self.shared.garbage_waits.fetch_add(1, Ordering::Relaxed);
        if let Some(slot) = self.retired.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(track);
        } else {
            // Only if more than 20 tracks are replaced before the cleanup
            // thread runs: leaking is safer than freeing on the audio thread.
            Box::leak(track);
            self.shared.leaked_tracks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Retries sending the kept tracks.
    fn hand_back_retired(&mut self) {
        for slot in &mut self.retired {
            if let Some(track) = slot.take()
                && let Err(rtrb::PushError::Full(track)) = self.garbage.push(track)
            {
                *slot = Some(track);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
