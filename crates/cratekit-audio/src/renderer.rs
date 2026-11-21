//! The real-time part: what runs inside the audio callback.
//!
//! [`Renderer::render`] fills the output buffer. It never allocates, frees,
//! locks, blocks, does I/O or panics (see the realtime rules in
//! docs/ARCHITECTURE.md, Threads): commands arrive over a lock-free queue,
//! replaced tracks and stems leave over another one to be freed elsewhere,
//! and position and meters go out through atomics.
//!
//! A deck plays its track, or its four stems mixed with a gain, mute and solo
//! each (#34). Switching between them crossfades, and every stem change is
//! ramped, over 10 ms, so nothing clicks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer};

use crate::filter::DjFilter;

/// Replaced tracks kept in the renderer when the garbage queue is full.
const RETIRED_SLOTS: usize = 4;

/// Fades for play, pause, seek and load last this long, so nothing clicks.
pub const FADE_SECONDS: f32 = 0.005;

/// Mode switches and stem gain, mute and solo changes ramp over this long.
pub const MIX_SECONDS: f32 = 0.010;

/// The stems of a track, in the model's order.
pub const STEM_COUNT: usize = 4;

/// Highest gain a stem can be given (twice its level).
pub const MAX_STEM_GAIN: f32 = 2.0;

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

/// One of a track's four stems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stem {
    /// Drums.
    Drums,
    /// Bass.
    Bass,
    /// Everything that is not drums, bass or vocals.
    Other,
    /// Vocals.
    Vocals,
}

impl Stem {
    /// Every stem, in the model's order.
    pub const ALL: [Self; STEM_COUNT] = [Self::Drums, Self::Bass, Self::Other, Self::Vocals];

    fn index(self) -> usize {
        match self {
            Self::Drums => 0,
            Self::Bass => 1,
            Self::Other => 2,
            Self::Vocals => 3,
        }
    }
}

/// A track's four stems ready to play: interleaved stereo `f32` at the
/// engine's rate, prepared off the audio thread, in the order of
/// [`Stem::ALL`].
#[derive(Debug, Clone, PartialEq)]
pub struct StemSet {
    sources: [Vec<f32>; STEM_COUNT],
}

impl StemSet {
    /// Stems from the left and right channels of each, in the order of
    /// [`Stem::ALL`].
    #[must_use]
    pub fn from_stereo(stems: [(&[f32], &[f32]); STEM_COUNT]) -> Self {
        Self {
            sources: stems.map(|(left, right)| Track::from_stereo(left, right).samples),
        }
    }

    /// Stems from prepared tracks, without copying their samples.
    pub(crate) fn from_tracks(tracks: [Track; STEM_COUNT]) -> Self {
        Self {
            sources: tracks.map(|track| track.samples),
        }
    }

    /// Number of stereo frames (the shortest stem's).
    #[must_use]
    pub fn frames(&self) -> usize {
        self.sources
            .iter()
            .map(|samples| samples.len() / 2)
            .min()
            .unwrap_or(0)
    }

    /// The stems at `index` summed with `gains`; silence past the end.
    fn mix(&self, index: usize, gains: &[f32; STEM_COUNT]) -> (f32, f32) {
        let start = index.saturating_mul(2);
        let mut sum = (0.0, 0.0);
        for (samples, &gain) in self.sources.iter().zip(gains) {
            if let Some([left, right]) = samples.get(start..start.saturating_add(2)) {
                sum.0 += left * gain;
                sum.1 += right * gain;
            }
        }
        sum
    }
}

/// What a deck plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackMode {
    /// The track itself.
    #[default]
    Original,
    /// Its stems, mixed (the track itself while no stems are attached).
    Stems,
}

/// Something the audio thread replaced, handed to another thread to be
/// freed there.
#[derive(Debug)]
pub enum Retired {
    /// A track.
    Track(Box<Track>),
    /// A track's stems.
    Stems(Box<StemSet>),
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
    /// Give the track its stems (allocated before it is sent). Sent after a
    /// `Load`, they wait for that track; the stems of a replaced track go
    /// with it.
    AttachStems(Box<StemSet>),
    /// Play the track or its stems (crossfaded).
    SetMode(PlaybackMode),
    /// A stem's gain, 0 to [`MAX_STEM_GAIN`] (ramped).
    SetStemGain(Stem, f32),
    /// Mute or unmute a stem (ramped).
    SetStemMute(Stem, bool),
    /// Solo or unsolo a stem: while any stem is soloed, exactly the soloed
    /// ones are heard, muted or not (ramped).
    SetStemSolo(Stem, bool),
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
    /// Replaced tracks or stems that could not be handed to the cleanup
    /// thread yet.
    pub garbage_waits: AtomicU64,
    /// Replaced tracks or stems leaked because every slot was full (never
    /// freed on the audio thread). Should stay 0.
    pub leaked_tracks: AtomicU64,
}

/// The audio callback's own state. It owns everything it touches.
pub struct Renderer {
    commands: Consumer<(DeckId, Command)>,
    garbage: Producer<Retired>,
    shared: Arc<Shared>,
    /// Output channels of the device (the track is stereo).
    channels: usize,
    /// Gain change per frame while fading.
    fade_step: f32,
    track: Option<Box<Track>>,
    /// A track waiting to replace the current one once the fade out ends.
    incoming: Option<Box<Track>>,
    /// Replaced tracks and stems the garbage queue had no room for yet.
    retired: [Option<Retired>; RETIRED_SLOTS],
    position: usize,
    playing: bool,
    gain: f32,
    target: f32,
    pending_seek: Option<usize>,
    filter: DjFilter,
    /// The current track's stems.
    stems: Option<Box<StemSet>>,
    /// Stems that arrived for the waiting track.
    incoming_stems: Option<Box<StemSet>>,
    mode: PlaybackMode,
    /// How much of the stems is heard, 0 (the track) to 1 (the stems).
    stems_mix: f32,
    /// Ramp step per frame for the mix and the stem gains.
    mix_step: f32,
    stem_gain_setting: [f32; STEM_COUNT],
    stem_muted: [bool; STEM_COUNT],
    stem_soloed: [bool; STEM_COUNT],
    /// Each stem's gain now, ramping toward its target.
    stem_gain: [f32; STEM_COUNT],
    stem_target: [f32; STEM_COUNT],
}

impl Renderer {
    /// A renderer for a device with `channels` outputs at `sample_rate`,
    /// its DJ filter already at `filter` (-1 to 1).
    #[must_use]
    pub fn new(
        commands: Consumer<(DeckId, Command)>,
        garbage: Producer<Retired>,
        shared: Arc<Shared>,
        channels: usize,
        sample_rate: u32,
        filter: f32,
    ) -> Self {
        let mut dj_filter = DjFilter::new(sample_rate);
        dj_filter.jump(filter);
        let fade_frames = (FADE_SECONDS * sample_rate as f32).max(1.0);
        let mix_frames = (MIX_SECONDS * sample_rate as f32).max(1.0);
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
            stems: None,
            incoming_stems: None,
            mode: PlaybackMode::Original,
            stems_mix: 0.0,
            mix_step: 1.0 / mix_frames,
            stem_gain_setting: [1.0; STEM_COUNT],
            stem_muted: [false; STEM_COUNT],
            stem_soloed: [false; STEM_COUNT],
            stem_gain: [1.0; STEM_COUNT],
            stem_target: [1.0; STEM_COUNT],
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
                // waiting one; the waiting one is handed back, with any stems
                // that came for it.
                if let Some(waiting) = self.incoming.replace(track) {
                    self.retire(Retired::Track(waiting));
                }
                if let Some(waiting) = self.incoming_stems.take() {
                    self.retire(Retired::Stems(waiting));
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
            Command::AttachStems(stems) => {
                let slot = if self.incoming.is_some() {
                    &mut self.incoming_stems
                } else {
                    &mut self.stems
                };
                if let Some(old) = slot.replace(stems) {
                    self.retire(Retired::Stems(old));
                }
            }
            Command::SetMode(mode) => self.mode = mode,
            Command::SetStemGain(stem, gain) => {
                let gain = if gain.is_finite() {
                    gain.clamp(0.0, MAX_STEM_GAIN)
                } else {
                    1.0
                };
                if let Some(setting) = self.stem_gain_setting.get_mut(stem.index()) {
                    *setting = gain;
                }
                self.update_stem_targets();
            }
            Command::SetStemMute(stem, muted) => {
                if let Some(flag) = self.stem_muted.get_mut(stem.index()) {
                    *flag = muted;
                }
                self.update_stem_targets();
            }
            Command::SetStemSolo(stem, soloed) => {
                if let Some(flag) = self.stem_soloed.get_mut(stem.index()) {
                    *flag = soloed;
                }
                self.update_stem_targets();
            }
        }
    }

    /// Each stem's target gain from its setting, mute and solo: while any
    /// stem is soloed, exactly the soloed ones are heard.
    fn update_stem_targets(&mut self) {
        let any_solo = self.stem_soloed.iter().any(|&soloed| soloed);
        let flags = self.stem_muted.iter().zip(&self.stem_soloed);
        for ((target, &setting), (&muted, &soloed)) in self
            .stem_target
            .iter_mut()
            .zip(&self.stem_gain_setting)
            .zip(flags)
        {
            let heard = if any_solo { soloed } else { !muted };
            *target = if heard { setting } else { 0.0 };
        }
    }

    /// Moves the stems mix and the stem gains one frame toward their targets.
    fn ramp_mix(&mut self) {
        let mix_target = if self.mode == PlaybackMode::Stems && self.stems.is_some() {
            1.0
        } else {
            0.0
        };
        self.stems_mix = step_toward(self.stems_mix, mix_target, self.mix_step);
        for (gain, &target) in self.stem_gain.iter_mut().zip(&self.stem_target) {
            *gain = step_toward(*gain, target, self.mix_step);
        }
    }

    /// The next output frame, moving the fade and the position.
    fn next_frame(&mut self) -> (f32, f32) {
        self.ramp_mix();
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
        let (left, right) = match self.stems.as_deref() {
            Some(stems) if self.stems_mix > 0.0 => {
                let (stem_left, stem_right) = stems.mix(self.position, &self.stem_gain);
                (
                    left + (stem_left - left) * self.stems_mix,
                    right + (stem_right - right) * self.stems_mix,
                )
            }
            _ => (left, right),
        };
        self.position += 1;
        (left * self.gain, right * self.gain)
    }

    /// At silence: swap in a waiting track, apply a waiting seek, then fade
    /// back in if playing.
    fn settle(&mut self) {
        if self.incoming.is_some() {
            if let Some(old) = std::mem::replace(&mut self.track, self.incoming.take()) {
                self.retire(Retired::Track(old));
            }
            // The old track's stems go with it; the new one's come in.
            if let Some(old) = std::mem::replace(&mut self.stems, self.incoming_stems.take()) {
                self.retire(Retired::Stems(old));
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

    /// Sends a replaced track or stems to the cleanup thread to be freed
    /// there. When the queue is full it is kept in a slot and sent later; it
    /// must never be freed here.
    fn retire(&mut self, item: Retired) {
        let Err(rtrb::PushError::Full(item)) = self.garbage.push(item) else {
            return;
        };
        self.shared.garbage_waits.fetch_add(1, Ordering::Relaxed);
        if let Some(slot) = self.retired.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(item);
        } else {
            // Only if more than 20 items are replaced before the cleanup
            // thread runs: leaking is safer than freeing on the audio thread.
            std::mem::forget(item);
            self.shared.leaked_tracks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Retries sending the kept tracks.
    fn hand_back_retired(&mut self) {
        for slot in &mut self.retired {
            if let Some(item) = slot.take()
                && let Err(rtrb::PushError::Full(item)) = self.garbage.push(item)
            {
                *slot = Some(item);
                return;
            }
        }
    }
}

/// `value` moved by at most `step` toward `target`.
fn step_toward(value: f32, target: f32, step: f32) -> f32 {
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

#[cfg(test)]
mod tests;
