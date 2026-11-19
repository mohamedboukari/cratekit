//! The playback engine: owns the output device and talks to the audio
//! callback through lock-free queues and atomics.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use cratekit_core::CoreError;
use cratekit_core::audio::{AudioBuffer, resample};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::renderer::{Command, DeckId, Renderer, Shared, Track};

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

/// The running playback engine. Dropping it closes the output.
pub struct Engine {
    commands: Mutex<Producer<(DeckId, Command)>>,
    shared: Arc<Shared>,
    device_error: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    sample_rate: u32,
    /// Closing this ends the audio thread (which then drops the stream).
    stop: Option<mpsc::Sender<()>>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    /// Opens the default output device and starts the audio callback, silent
    /// until a track is loaded and played.
    ///
    /// The cpal stream lives on its own thread: on some systems it cannot
    /// move between threads, and the app's state must.
    ///
    /// # Errors
    ///
    /// [`EngineError::NoDevice`] without an output, [`EngineError::Device`]
    /// when the device cannot be opened.
    pub fn start() -> Result<Self, EngineError> {
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
                match open_stream(command_queue, garbage_queue, audio_shared, audio_error) {
                    Ok((stream, rate)) => {
                        ready.send(Ok(rate)).ok();
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
        let sample_rate = started
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
            stop: Some(stop),
            threads: vec![audio, cleanup],
        })
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
        Ok(prepare(audio, self.sample_rate)?)
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
        self.stopping.store(true, Ordering::Relaxed);
        // Closing the channel wakes the audio thread, which drops the stream.
        self.stop.take();
        for thread in self.threads.drain(..) {
            thread.join().ok();
        }
    }
}

/// Opens the default output and starts a stream that runs `Renderer::render`.
///
/// Renders in `f32`. Uses the device's default format when it is `f32`, else
/// an `f32` format the device also offers, else converts to its default
/// integer format at this boundary.
fn open_stream(
    commands: Consumer<(DeckId, Command)>,
    garbage: Producer<Box<Track>>,
    shared: Arc<Shared>,
    device_error: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32), EngineError> {
    let failed = |err: &dyn std::fmt::Display| EngineError::Device(err.to_string());
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(EngineError::NoDevice)?;
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
    let config = chosen.config();
    let rate = config.sample_rate;
    let renderer = Renderer::new(
        commands,
        garbage,
        shared,
        usize::from(config.channels),
        rate,
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
    Ok((stream, rate))
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
        // Called from an audio context too: only set a flag.
        move |_| device_error.store(true, Ordering::Relaxed),
        None,
    )
}

/// Frees the tracks the audio thread replaced, off the audio thread, until
/// the engine stops.
fn free_replaced_tracks(mut garbage: Consumer<Box<Track>>, stopping: &AtomicBool) {
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
}
