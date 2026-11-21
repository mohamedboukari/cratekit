//! The playback engine: owns the output device and talks to the audio
//! callback through lock-free queues and atomics.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};
use cratekit_core::CoreError;
use cratekit_core::audio::{AudioBuffer, resample};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::renderer::{
    Command, DeckId, MasterPreview, PlaybackMode, Renderer, Retired, STEM_COUNT, Shared, Stem,
    StemSet, Track,
};

/// Commands waiting for the audio thread (it reads them every callback).
const COMMAND_CAPACITY: usize = 64;
/// Replaced tracks waiting for the cleanup thread.
const GARBAGE_CAPACITY: usize = 16;
/// How often the cleanup thread frees replaced tracks.
const CLEANUP_INTERVAL: Duration = Duration::from_millis(50);
/// Frames rendered at a time before converting to the device's sample
/// format (the buffer is allocated once, before the stream starts).
const CONVERT_FRAMES: usize = 1024;

/// Why the engine could not do something.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The system has no audio output.
    #[error("no audio output device")]
    NoDevice,
    /// The output device refused or failed.
    #[error("the audio output failed: {0}")]
    Device(String),
    /// The command queue is full (the audio thread is not running).
    #[error("the audio engine is not responding")]
    Busy,
    /// Decoding or resampling the track failed.
    #[error(transparent)]
    Audio(#[from] CoreError),
}

/// What the engine is doing, for the 30 Hz events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    /// Playback position in seconds.
    pub position_sec: f64,
    /// Whether it is playing.
    pub playing: bool,
    /// How many times a track has played to its end so far.
    pub ends: u64,
    /// Highest absolute output sample since the last status (0 to 1).
    pub peak: f32,
    /// RMS of the latest output block (0 to 1).
    pub rms: f32,
    /// Whether the device reported an error (unplugged, for example).
    pub device_error: bool,
}

/// An audio output the system offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// Stable id, to choose it again later (stored in the settings).
    pub id: String,
    /// Name to show.
    pub name: String,
    /// Whether it is the system's default output.
    pub is_default: bool,
    /// Its default sample rate (0 when the device does not say).
    pub sample_rate: u32,
}

/// Every output device, the system default marked.
///
/// # Errors
///
/// [`EngineError::Device`] when the system cannot list its devices.
pub fn output_devices() -> Result<Vec<OutputDevice>, EngineError> {
    let host = cpal::default_host();
    let default = default_device_id();
    let devices = host
        .output_devices()
        .map_err(|err| EngineError::Device(err.to_string()))?;
    Ok(devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            if is_unstable_id(&id) {
                return None;
            }
            let name = device
                .description()
                .map_or_else(|_| id.clone(), |description| description.name().to_owned());
            let sample_rate = device
                .default_output_config()
                .map_or(0, |config| config.sample_rate());
            Some(OutputDevice {
                is_default: default.as_deref() == Some(id.as_str()),
                id,
                name,
                sample_rate,
            })
        })
        .collect())
}

/// The ids of the output devices, without opening or probing them (cheap
/// enough to check every few seconds).
///
/// # Errors
///
/// [`EngineError::Device`] when the system cannot list its devices.
pub fn output_device_ids() -> Result<Vec<String>, EngineError> {
    let devices = cpal::default_host()
        .output_devices()
        .map_err(|err| EngineError::Device(err.to_string()))?;
    Ok(devices
        .filter_map(|device| device.id().ok().map(|id| id.to_string()))
        .filter(|id| !is_unstable_id(id))
        .collect())
}

/// Whether a device id names a sound card by its number (ALSA's
/// `hw:CARD=0,DEV=0`): the number can point to another card after a reboot
/// or a replug, so it must not be stored. Every such card also has a
/// name-based id (`hw:CARD=USB,DEV=0`), which is kept.
fn is_unstable_id(id: &str) -> bool {
    id.split("CARD=").nth(1).is_some_and(|rest| {
        let card: String = rest.chars().take_while(|c| *c != ',').collect();
        !card.is_empty() && card.chars().all(|c| c.is_ascii_digit())
    })
}

/// Whether a stream error means the output is gone or broken (the engine
/// must reopen it), as opposed to a passing glitch: an xrun, a refused
/// real-time priority, a busy device, or the system rerouting the default
/// output (cpal follows that by itself).
#[must_use]
pub fn is_fatal(kind: cpal::ErrorKind) -> bool {
    !matches!(
        kind,
        cpal::ErrorKind::Xrun
            | cpal::ErrorKind::DeviceChanged
            | cpal::ErrorKind::RealtimeDenied
            | cpal::ErrorKind::DeviceBusy
    )
}

/// Makes a decoded track ready for an output at `rate`: stereo (a mono
/// track on both sides, the first two channels of anything wider), at that
/// rate, with any non-finite sample (from a damaged file) silenced. Needs no
/// engine, so it can run while the engine restarts.
///
/// # Errors
///
/// [`EngineError::Audio`] if resampling fails.
pub fn prepare_track(audio: AudioBuffer, rate: u32) -> Result<Track, EngineError> {
    Ok(prepare(audio, rate)?)
}

/// Makes a track's split stems ready for an engine at `rate`, each as
/// [`prepare_track`] does, in the order of [`Stem::ALL`]. Needs no engine.
///
/// # Errors
///
/// [`EngineError::Audio`] if resampling fails.
pub fn prepare_stems(
    stems: cratekit_core::stems::Stems,
    rate: u32,
) -> Result<StemSet, EngineError> {
    let cratekit_core::stems::Stems {
        drums,
        bass,
        other,
        vocals,
    } = stems;
    let tracks: [Track; STEM_COUNT] = [
        prepare(drums, rate)?,
        prepare(bass, rate)?,
        prepare(other, rate)?,
        prepare(vocals, rate)?,
    ];
    Ok(StemSet::from_tracks(tracks))
}

/// The id of the system's default output, if there is one.
#[must_use]
pub fn default_device_id() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string())
        .filter(|id| !is_unstable_id(id))
}

/// How to open the output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineOptions {
    /// The device to use (an [`OutputDevice::id`]); `None`, or a device that
    /// is gone, means the system default.
    pub device_id: Option<String>,
    /// Buffer size in frames, used when the device allows it.
    pub buffer_size: Option<u32>,
    /// Where the DJ filter knob starts (-1 to 1, 0 off). The engine starts
    /// there at once, with no sweep (see [`Engine::set_filter`]).
    pub filter: f32,
}

/// The running playback engine. Dropping it closes the output.
pub struct Engine {
    commands: Mutex<Producer<(DeckId, Command)>>,
    shared: Arc<Shared>,
    device_error: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    sample_rate: u32,
    /// The device actually opened.
    device_id: String,
    /// The buffer size asked for.
    buffer_size: Option<u32>,
    /// Closing this ends the audio thread (which then drops the stream).
    stop: Mutex<Option<mpsc::Sender<()>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Engine {
    /// Opens the output (the chosen device, else the system default) and
    /// starts the audio callback, silent until a track is loaded and played.
    ///
    /// The cpal stream lives on its own thread: on some systems it cannot
    /// move between threads, and the app's state must.
    ///
    /// # Errors
    ///
    /// [`EngineError::NoDevice`] without an output, [`EngineError::Device`]
    /// when the device cannot be opened.
    pub fn start(options: &EngineOptions) -> Result<Self, EngineError> {
        match Self::start_on(options) {
            Ok(engine) => Ok(engine),
            // The chosen device is there but refuses to open: try the default.
            Err(err) if options.device_id.is_some() => {
                let default = EngineOptions {
                    device_id: None,
                    ..options.clone()
                };
                Self::start_on(&default).map_err(|_| err)
            }
            Err(err) => Err(err),
        }
    }

    fn start_on(options: &EngineOptions) -> Result<Self, EngineError> {
        let buffer_size = options.buffer_size;
        let options = options.clone();
        let (commands, command_queue) = RingBuffer::new(COMMAND_CAPACITY);
        let (garbage_queue, garbage) = RingBuffer::new(GARBAGE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let device_error = Arc::new(AtomicBool::new(false));
        let stopping = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();

        let audio_shared = Arc::clone(&shared);
        let audio_error = Arc::clone(&device_error);
        let audio = thread::Builder::new()
            .name("cratekit-audio".into())
            .spawn(move || {
                match open_stream(
                    command_queue,
                    garbage_queue,
                    audio_shared,
                    audio_error,
                    &options,
                ) {
                    Ok((stream, opened)) => {
                        ready.send(Ok(opened)).ok();
                        // Keep the stream until the engine is dropped.
                        stopped.recv().ok();
                        drop(stream);
                    }
                    Err(err) => {
                        ready.send(Err(err)).ok();
                    }
                }
            })
            .map_err(|err| EngineError::Device(err.to_string()))?;
        let (sample_rate, device_id) = started
            .recv()
            .map_err(|_| EngineError::Device("the audio thread stopped".into()))??;

        let cleanup_stopping = Arc::clone(&stopping);
        let cleanup = thread::Builder::new()
            .name("cratekit-audio-cleanup".into())
            .spawn(move || free_replaced_tracks(garbage, &cleanup_stopping))
            .map_err(|err| EngineError::Device(err.to_string()))?;

        Ok(Self {
            commands: Mutex::new(commands),
            shared,
            device_error,
            stopping,
            sample_rate,
            device_id,
            buffer_size,
            stop: Mutex::new(Some(stop)),
            threads: Mutex::new(vec![audio, cleanup]),
        })
    }

    /// The id of the device it opened (the system default when the chosen
    /// one was missing).
    #[must_use]
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The buffer size it was asked to use (the device may not allow it).
    #[must_use]
    pub fn buffer_size(&self) -> Option<u32> {
        self.buffer_size
    }

    /// Closes the output now, even while other handles to the engine are
    /// alive: stops the stream and waits for its threads. Commands sent
    /// afterwards go nowhere. Closing twice does nothing.
    pub fn close(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        // Closing the channel wakes the audio thread, which drops the stream.
        self.stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let threads: Vec<_> = self
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        for thread in threads {
            thread.join().ok();
        }
    }

    /// Whether [`close`](Self::close) was called.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }

    /// The device's sample rate; tracks are resampled to it.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Makes a decoded track ready for this engine: stereo (a mono track on
    /// both sides, the first two channels of anything wider), at the device
    /// rate, with any non-finite sample (from a damaged file) silenced. Runs
    /// on the calling thread; it can take a moment for a long track.
    ///
    /// # Errors
    ///
    /// [`EngineError::Audio`] if resampling fails.
    pub fn prepare(&self, audio: AudioBuffer) -> Result<Track, EngineError> {
        prepare_track(audio, self.sample_rate)
    }

    /// Puts a prepared track on `deck`, ready to play from the start:
    /// stopped, at position 0.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn load(&self, deck: DeckId, track: Track) -> Result<(), EngineError> {
        self.send(deck, Command::Load(Box::new(track)))
    }

    /// Plays `deck` from its current position.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn play(&self, deck: DeckId) -> Result<(), EngineError> {
        self.send(deck, Command::Play)
    }

    /// Pauses `deck`, keeping the position.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn pause(&self, deck: DeckId) -> Result<(), EngineError> {
        self.send(deck, Command::Pause)
    }

    /// Jumps `deck` to `seconds` from the start (clamped to the track).
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn seek(&self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        let frame = (seconds.max(0.0) * f64::from(self.sample_rate)).round() as u64;
        self.send(deck, Command::Seek(frame))
    }

    /// Turns the DJ filter of `deck`: -1 full low-pass, 0 off, 1 full
    /// high-pass. Out-of-range values are clamped, and one that is not a
    /// number counts as 0. The change is smoothed on the audio thread.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_filter(&self, deck: DeckId, value: f32) -> Result<(), EngineError> {
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.send(deck, Command::SetFilter(value))
    }

    /// Gives the track on `deck` its stems (prepared with [`prepare_stems`]).
    /// Sent right after [`Self::load`], they wait for that track.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn attach_stems(&self, deck: DeckId, stems: StemSet) -> Result<(), EngineError> {
        self.send(deck, Command::AttachStems(Box::new(stems)))
    }

    /// Plays the track on `deck` or its stems (crossfaded over 10 ms; the
    /// track while no stems are attached).
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_mode(&self, deck: DeckId, mode: PlaybackMode) -> Result<(), EngineError> {
        self.send(deck, Command::SetMode(mode))
    }

    /// Sets the mastered side of A/B (#26): heard in [`PlaybackMode::Mastered`].
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_master(&self, deck: DeckId, preview: MasterPreview) -> Result<(), EngineError> {
        self.send(deck, Command::SetMaster(preview))
    }

    /// Level match on or off for the mastered side (on by default).
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_level_match(&self, deck: DeckId, on: bool) -> Result<(), EngineError> {
        self.send(deck, Command::SetLevelMatch(on))
    }

    /// A stem's gain, 0 to 2 (clamped; not a number counts as 1), ramped.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_stem_gain(&self, deck: DeckId, stem: Stem, gain: f32) -> Result<(), EngineError> {
        self.send(deck, Command::SetStemGain(stem, gain))
    }

    /// Mutes or unmutes a stem, ramped.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_stem_mute(&self, deck: DeckId, stem: Stem, muted: bool) -> Result<(), EngineError> {
        self.send(deck, Command::SetStemMute(stem, muted))
    }

    /// Solos or unsolos a stem (while any stem is soloed, exactly the soloed
    /// ones are heard), ramped.
    ///
    /// # Errors
    ///
    /// [`EngineError::Busy`] if the audio thread does not take commands.
    pub fn set_stem_solo(&self, deck: DeckId, stem: Stem, soloed: bool) -> Result<(), EngineError> {
        self.send(deck, Command::SetStemSolo(stem, soloed))
    }

    /// Position, playing state and meters, read from the audio thread's
    /// atomics. Reading resets the peak, so each status has the peak since
    /// the previous one.
    #[must_use]
    pub fn status(&self, deck: DeckId) -> Status {
        // One deck so far; a second one gets its own `Shared`.
        let DeckId::A = deck;
        let frames = self.shared.position.load(Ordering::Relaxed);
        Status {
            position_sec: frames as f64 / f64::from(self.sample_rate),
            playing: self.shared.playing.load(Ordering::Relaxed),
            ends: self.shared.ends.load(Ordering::Relaxed),
            peak: f32::from_bits(self.shared.peak.swap(0, Ordering::Relaxed)),
            rms: f32::from_bits(self.shared.rms.load(Ordering::Relaxed)),
            device_error: self.device_error.load(Ordering::Relaxed),
        }
    }

    fn send(&self, deck: DeckId, command: Command) -> Result<(), EngineError> {
        let mut commands = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        commands
            .push((deck, command))
            .map_err(|_| EngineError::Busy)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

/// Opens the output `options` asks for (else the default) and starts a
/// stream that runs `Renderer::render`. Returns the stream, its sample rate
/// and the id of the device it opened.
///
/// Renders in `f32`. Uses the device's default format when it is `f32`, else
/// an `f32` format the device also offers, else converts to its default
/// integer format at this boundary.
fn open_stream(
    commands: Consumer<(DeckId, Command)>,
    garbage: Producer<Retired>,
    shared: Arc<Shared>,
    device_error: Arc<AtomicBool>,
    options: &EngineOptions,
) -> Result<(cpal::Stream, (u32, String)), EngineError> {
    let failed = |err: &dyn std::fmt::Display| EngineError::Device(err.to_string());
    let host = cpal::default_host();
    let chosen_device = options.device_id.as_deref().and_then(|wanted| {
        host.output_devices()
            .ok()?
            .find(|device| device.id().is_ok_and(|id| id.to_string() == wanted))
    });
    let device = match chosen_device {
        Some(device) => device,
        None => host.default_output_device().ok_or(EngineError::NoDevice)?,
    };
    let device_id = device
        .id()
        .map(|id| id.to_string())
        .map_err(|err| failed(&err))?;
    let default = device.default_output_config().map_err(|err| failed(&err))?;
    let chosen = if default.sample_format() == SampleFormat::F32 {
        default
    } else {
        device
            .supported_output_configs()
            .ok()
            .and_then(|mut ranges| {
                ranges.find(|range| {
                    range.sample_format() == SampleFormat::F32
                        && range.channels() == default.channels()
                })
            })
            .map(|range| {
                range
                    .try_with_sample_rate(default.sample_rate())
                    .unwrap_or_else(|| range.with_max_sample_rate())
            })
            .unwrap_or(default)
    };
    let format = chosen.sample_format();
    let buffer_range = match chosen.buffer_size() {
        SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        SupportedBufferSize::Unknown => None,
    };
    let mut config = chosen.config();
    if let Some(frames) = options.buffer_size
        && buffer_range.is_some_and(|(min, max)| (min..=max).contains(&frames))
    {
        config.buffer_size = BufferSize::Fixed(frames);
    }
    let rate = config.sample_rate;
    let renderer = Renderer::new(
        commands,
        garbage,
        shared,
        usize::from(config.channels),
        rate,
        options.filter,
    );
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, renderer, device_error),
        SampleFormat::I16 => build::<i16>(&device, config, renderer, device_error),
        SampleFormat::I32 => build::<i32>(&device, config, renderer, device_error),
        SampleFormat::U16 => build::<u16>(&device, config, renderer, device_error),
        other => {
            return Err(EngineError::Device(format!(
                "unsupported sample format {other}"
            )));
        }
    }
    .map_err(|err| failed(&err))?;
    stream.play().map_err(|err| failed(&err))?;
    Ok((stream, (rate, device_id)))
}

/// Builds an output stream in sample type `T`: the renderer fills a scratch
/// `f32` buffer (allocated here, before the stream starts) in chunks, and
/// each sample is converted to `T`. For `f32` the conversion is a copy.
fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: Renderer,
    device_error: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let mut scratch = vec![0.0_f32; CONVERT_FRAMES * usize::from(config.channels).max(1)];
    device.build_output_stream::<T, _, _>(
        config,
        move |out: &mut [T], _| {
            for chunk in out.chunks_mut(scratch.len()) {
                let part = scratch.get_mut(..chunk.len()).unwrap_or_default();
                renderer.render(part);
                for (sample, value) in chunk.iter_mut().zip(part.iter()) {
                    *sample = T::from_sample(*value);
                }
            }
        },
        // Called from an audio context too: only set a flag, and only for an
        // error that means the output is gone (an xrun is not one).
        move |err: cpal::Error| {
            if is_fatal(err.kind()) {
                device_error.store(true, Ordering::Relaxed);
            }
        },
        None,
    )
}

/// Frees the tracks and stems the audio thread replaced, off the audio
/// thread, until the engine stops.
fn free_replaced_tracks(mut garbage: Consumer<Retired>, stopping: &AtomicBool) {
    loop {
        while garbage.pop().is_ok() {}
        if stopping.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(CLEANUP_INTERVAL);
    }
}

/// A decoded track as the renderer plays it: stereo at `rate`. Takes the
/// track by value so its channels are reused, not copied: at most the input
/// and the resampled output are alive at once.
fn prepare(audio: AudioBuffer, rate: u32) -> Result<Track, CoreError> {
    let source_rate = audio.sample_rate();
    let mut channels = audio.into_channels();
    if channels.len() == 1 {
        let mono = channels.first().cloned().unwrap_or_default();
        channels.push(mono);
    }
    channels.truncate(2);
    // A damaged file can hold NaN or infinite samples: silence them before
    // the resampler spreads them.
    for channel in &mut channels {
        for sample in channel.iter_mut().filter(|sample| !sample.is_finite()) {
            *sample = 0.0;
        }
    }
    let resampled = resample(AudioBuffer::new(source_rate, channels)?, rate)?;
    match resampled.channels() {
        [left, right] => Ok(Track::from_stereo(left, right)),
        _ => Err(CoreError::InvalidAudio("expected two channels")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_any_track_as_stereo_at_the_device_rate() {
        let mono = AudioBuffer::new(44_100, vec![vec![0.5; 44_100]]).unwrap();
        let track = prepare(mono, 48_000).unwrap();
        assert_eq!(track.frames(), 48_000);

        let wide =
            AudioBuffer::new(48_000, vec![vec![0.1; 10], vec![0.2; 10], vec![0.9; 10]]).unwrap();
        let track = prepare(wide, 48_000).unwrap();
        assert_eq!(track, Track::from_stereo(&[0.1; 10], &[0.2; 10]));
    }

    #[test]
    fn damaged_samples_are_silenced_before_playback() {
        let mut left = vec![0.25_f32; 4_800];
        left[100] = f32::NAN;
        left[200] = f32::INFINITY;
        for rate in [48_000, 44_100] {
            let audio = AudioBuffer::new(48_000, vec![left.clone(), left.clone()]).unwrap();
            let track = prepare(audio, rate).unwrap();
            assert!(
                track.samples().iter().all(|s| s.is_finite()),
                "at {rate} Hz"
            );
        }
    }

    #[test]
    fn only_errors_that_mean_the_output_is_gone_restart_it() {
        for kind in [
            cpal::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::StreamInvalidated,
            cpal::ErrorKind::HostUnavailable,
            cpal::ErrorKind::BackendError,
            cpal::ErrorKind::Other,
        ] {
            assert!(is_fatal(kind), "{kind:?}");
        }
        for kind in [
            cpal::ErrorKind::Xrun,
            cpal::ErrorKind::DeviceChanged,
            cpal::ErrorKind::RealtimeDenied,
            cpal::ErrorKind::DeviceBusy,
        ] {
            assert!(!is_fatal(kind), "{kind:?}");
        }
    }

    #[test]
    fn card_numbers_are_not_stable_device_ids() {
        for id in ["alsa:hw:CARD=0,DEV=0", "alsa:plughw:CARD=12,DEV=3"] {
            assert!(is_unstable_id(id), "{id}");
        }
        for id in [
            "alsa:hw:CARD=USB,DEV=0",
            "alsa:default",
            "coreaudio:BuiltInSpeakerDevice",
            "wasapi:{0.0.0.00000000}.{abc}",
        ] {
            assert!(!is_unstable_id(id), "{id}");
        }
    }
}
