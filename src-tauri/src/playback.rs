//! Playback commands and the 30 Hz position and meter events.
//!
//! The engine (`cratekit_audio::Engine`) starts on the first load, so the app
//! opens the audio device only when something is played. A poller thread
//! reads its atomics about 30 times a second and sends `playback_position`,
//! `meter` and, when a track reaches its end, `playback_state`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use cratekit_audio::{DeckId, Engine, EngineError, Status};
use cratekit_core::audio::decode_file;
use cratekit_core::library::TrackId;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppError;
use crate::library::LibraryState;

/// How often position and meters are sent: about 30 times a second.
const POLL_INTERVAL: Duration = Duration::from_millis(33);
/// Meters below this read as silence.
const SILENCE_DB: f32 = -96.0;

impl From<EngineError> for AppError {
    fn from(err: EngineError) -> Self {
        let code = match &err {
            EngineError::NoDevice => "no_audio_device",
            EngineError::Device(_) | EngineError::Busy => "audio_device",
            EngineError::Audio(core) => return Self::from_core(core, err.to_string()),
        };
        Self {
            code,
            message: err.to_string(),
        }
    }
}

/// The playback engine, started on first use, and the load counter.
#[derive(Default)]
pub(crate) struct PlayerState {
    engine: Mutex<Option<Arc<Engine>>>,
    /// Counts load requests: a load that finishes decoding after a newer one
    /// was asked for is dropped, so the deck always plays the latest track.
    loads: AtomicU64,
    /// Held while checking the counter and sending a track, so an older load
    /// can never land after a newer one.
    load_gate: Mutex<()>,
}

impl PlayerState {
    /// The running engine, starting it (opening the output device) the first
    /// time.
    fn engine(&self) -> Result<Arc<Engine>, AppError> {
        let mut engine = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(running) = engine.as_ref() {
            return Ok(Arc::clone(running));
        }
        let started = Arc::new(Engine::start()?);
        *engine = Some(Arc::clone(&started));
        Ok(started)
    }

    /// The engine if it is running (the poller never starts it).
    fn running(&self) -> Option<Arc<Engine>> {
        self.engine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// `playback_position`: the UI's position event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
struct Position {
    sec: f64,
}

/// `meter`: the UI's `MeterReading`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Meter {
    peak_db: f32,
    rms_db: f32,
}

/// `playback_state`: the UI's `PlaybackState`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
struct PlaybackState {
    playing: bool,
}

/// A level from 0 to 1 in dBFS, floored at `SILENCE_DB`.
fn to_db(level: f32) -> f32 {
    if level > 0.0 {
        (20.0 * level.log10()).max(SILENCE_DB)
    } else {
        SILENCE_DB
    }
}

/// What the poller sends after reading `status`, given the previous one.
fn events(previous: Option<&Status>, status: &Status) -> (Option<Position>, Option<Meter>, bool) {
    let moved = previous.is_none_or(|before| before.position_sec != status.position_sec);
    let position = (status.playing || moved).then_some(Position {
        sec: status.position_sec,
    });
    let sounding = status.playing || status.peak > 0.0;
    let meter = sounding.then_some(Meter {
        peak_db: to_db(status.peak),
        rms_db: to_db(status.rms),
    });
    let ended = previous.is_some_and(|before| status.ends > before.ends);
    (position, meter, ended)
}

/// Starts the poller thread, for as long as the app runs.
pub(crate) fn start_poller<R: Runtime>(app: AppHandle<R>) {
    thread::spawn(move || {
        let mut previous: Option<Status> = None;
        loop {
            thread::sleep(POLL_INTERVAL);
            let Some(engine) = app.state::<PlayerState>().running() else {
                continue;
            };
            let status = engine.status(DeckId::A);
            let (position, meter, ended) = events(previous.as_ref(), &status);
            // A window that is closing cannot receive events: nothing to do.
            if let Some(position) = position {
                app.emit("playback_position", position).ok();
            }
            if let Some(meter) = meter {
                app.emit("meter", meter).ok();
            }
            if ended {
                app.emit("playback_state", PlaybackState { playing: false })
                    .ok();
            }
            previous = Some(status);
        }
    });
}

/// Loads a track from the library: decodes it, resamples it to the device
/// and puts it on the deck, stopped at the start. When another load is asked
/// for meanwhile, this one stops without touching the deck (the UI ignores
/// its stale answer too).
#[tauri::command]
pub(crate) async fn playback_load(app: AppHandle, id: TrackId) -> Result<(), AppError> {
    let generation = app
        .state::<PlayerState>()
        .loads
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let player = app.state::<PlayerState>();
        let current = || player.loads.load(Ordering::SeqCst) == generation;
        let path = {
            let library = app.state::<LibraryState>();
            let library = library.library()?;
            let detail = library
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .track(id)?;
            detail.path
        };
        let audio = decode_file(&path)?;
        if !current() {
            return Ok(());
        }
        let engine = player.engine()?;
        let track = engine.prepare(audio)?;
        let _gate = player
            .load_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if current() {
            engine.load(DeckId::A, track)?;
        }
        Ok(())
    })
    .await
    .map_err(|err| AppError {
        code: "internal",
        message: err.to_string(),
    })?
}

/// Plays the loaded track.
#[tauri::command]
pub(crate) fn playback_play(player: tauri::State<'_, PlayerState>) -> Result<(), AppError> {
    Ok(player.engine()?.play(DeckId::A)?)
}

/// Pauses.
#[tauri::command]
pub(crate) fn playback_pause(player: tauri::State<'_, PlayerState>) -> Result<(), AppError> {
    Ok(player.engine()?.pause(DeckId::A)?)
}

/// Jumps to `sec` from the start.
#[tauri::command]
pub(crate) fn playback_seek(
    player: tauri::State<'_, PlayerState>,
    sec: f64,
) -> Result<(), AppError> {
    Ok(player.engine()?.seek(DeckId::A, sec)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(position_sec: f64, playing: bool, ends: u64, peak: f32) -> Status {
        Status {
            position_sec,
            playing,
            ends,
            peak,
            rms: peak / 2.0,
            device_error: false,
        }
    }

    #[test]
    fn sends_position_and_meters_while_playing() {
        let (position, meter, ended) = events(
            Some(&status(1.0, true, 0, 0.5)),
            &status(1.033, true, 0, 0.5),
        );
        assert_eq!(position, Some(Position { sec: 1.033 }));
        let meter = meter.unwrap();
        assert!((meter.peak_db - -6.02).abs() < 0.01);
        assert!(!ended);
    }

    #[test]
    fn a_seek_while_paused_still_moves_the_playhead_and_then_it_goes_quiet() {
        let paused = status(10.0, false, 0, 0.0);
        let (position, meter, _) = events(Some(&paused), &status(42.0, false, 0, 0.0));
        assert_eq!(position, Some(Position { sec: 42.0 }));
        assert_eq!(meter, None);
        let (position, _, _) = events(
            Some(&status(42.0, false, 0, 0.0)),
            &status(42.0, false, 0, 0.0),
        );
        assert_eq!(position, None, "nothing changed, nothing sent");
    }

    #[test]
    fn reports_each_end_of_track_once() {
        let (_, _, ended) = events(
            Some(&status(99.0, true, 0, 0.1)),
            &status(100.0, false, 1, 0.0),
        );
        assert!(ended);
        let (_, _, ended) = events(
            Some(&status(100.0, false, 1, 0.0)),
            &status(100.0, false, 1, 0.0),
        );
        assert!(!ended);
    }

    #[test]
    fn levels_convert_to_dbfs_with_a_floor() {
        assert!((to_db(1.0) - 0.0).abs() < 1e-6);
        assert!((to_db(0.1) - -20.0).abs() < 1e-4);
        assert_eq!(to_db(0.0), SILENCE_DB);
        assert_eq!(to_db(1e-9), SILENCE_DB);
    }

    #[test]
    fn events_serialize_as_the_ui_expects() {
        assert_eq!(
            serde_json::to_value(Position { sec: 1.5 }).unwrap(),
            serde_json::json!({ "sec": 1.5 })
        );
        assert_eq!(
            serde_json::to_value(Meter {
                peak_db: -3.0,
                rms_db: -9.0
            })
            .unwrap(),
            serde_json::json!({ "peakDb": -3.0, "rmsDb": -9.0 })
        );
        assert_eq!(
            serde_json::to_value(PlaybackState { playing: false }).unwrap(),
            serde_json::json!({ "playing": false })
        );
    }
}
