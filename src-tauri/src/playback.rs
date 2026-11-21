//! Playback commands, the output device, and the 30 Hz position and meter
//! events.
//!
//! The engine (`cratekit_audio::Engine`) starts on the first load, on the
//! output device and buffer size from the settings, so the app opens the
//! audio device only when something is played. A poller thread reads its
//! atomics about 30 times a second and sends `playback_position`, `meter`
//! and, when a track reaches its end, `playback_state`. It also watches the
//! device: when the output disappears, or the chosen or default device
//! changes, the engine restarts on the right device and keeps the track,
//! position and playing state (`audio_device_changed`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use cratekit_audio::{
    DeckId, Engine, EngineError, EngineOptions, OutputDevice, PlaybackMode, Status, Stem, StemSet,
    Track, default_device_id, output_device_ids, output_devices, prepare_stems, prepare_track,
};
use cratekit_core::audio::{AudioBuffer, decode_file};
use cratekit_core::library::TrackId;
use cratekit_core::stems::cache::read_folder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppError;
use crate::library::LibraryState;

/// How often position and meters are sent: about 30 times a second.
const POLL_INTERVAL: Duration = Duration::from_millis(33);
/// How many polls between two checks of the device list (about 2 s).
const DEVICE_CHECK_POLLS: u32 = 60;
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The guarded values stay valid if a panic interrupted a holder.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The playback engine (started on first use), the tracks on and coming to
/// the deck, and the load counter.
#[derive(Default)]
pub(crate) struct PlayerState {
    engine: Mutex<Option<Arc<Engine>>>,
    /// The track the engine holds.
    on_deck: Mutex<Option<TrackId>>,
    /// The track asked for last (its load may still be decoding).
    requested: Mutex<Option<TrackId>>,
    /// Counts load requests: a load that finishes decoding after a newer one
    /// was asked for is dropped, so the deck always plays the latest track.
    loads: AtomicU64,
    /// Held while the engine starts or restarts, while a track is sent to
    /// it, and around play, pause and seek: they then never act on an engine
    /// that is being replaced, and an older load never lands after a newer
    /// one.
    load_gate: Mutex<()>,
    /// The DJ filter knob (-1 to 1), given again to every engine that
    /// starts, so it survives a device change.
    filter: Mutex<f32>,
    /// What the deck plays and the stem mixer's settings, given again to
    /// every track put on the deck and every engine that starts (#34).
    mix: Mutex<StemMix>,
    /// The track whose stems the engine holds, if any.
    stems_on_deck: Mutex<Option<TrackId>>,
}

/// The playback mode and each stem's gain, mute and solo, in the order of
/// [`Stem::ALL`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct StemMix {
    mode: PlaybackMode,
    gain: [f32; 4],
    muted: [bool; 4],
    soloed: [bool; 4],
}

impl Default for StemMix {
    fn default() -> Self {
        Self {
            mode: PlaybackMode::Original,
            gain: [1.0; 4],
            muted: [false; 4],
            soloed: [false; 4],
        }
    }
}

impl PlayerState {
    /// The DJ filter knob position.
    fn filter(&self) -> f32 {
        *lock(&self.filter)
    }

    /// The engine if it is running.
    fn running(&self) -> Option<Arc<Engine>> {
        lock(&self.engine).clone()
    }

    /// The running engine, starting it (on the output and buffer size from
    /// the settings) when there is none. Call with `load_gate` held. A fresh
    /// engine gets back the track that was on the deck (stopped at the
    /// start), so Play never plays an empty deck after a failed restart.
    fn engine_locked<R: Runtime>(&self, app: &AppHandle<R>) -> Result<Arc<Engine>, AppError> {
        if let Some(running) = self.running() {
            return Ok(running);
        }
        let started = Arc::new(Engine::start(&engine_options(app))?);
        *lock(&self.engine) = Some(Arc::clone(&started));
        *lock(&self.stems_on_deck) = None;
        let deck = *lock(&self.on_deck);
        if let Some(id) = deck
            && put_on_deck(app, &started, id, None, 0.0, false).is_err()
        {
            *lock(&self.on_deck) = None;
        }
        Ok(started)
    }
}

/// The output device and buffer size from the settings (the defaults when
/// the library cannot be read).
fn engine_options<R: Runtime>(app: &AppHandle<R>) -> EngineOptions {
    let settings = app
        .state::<LibraryState>()
        .library()
        .ok()
        .and_then(|library| lock(library).settings().ok())
        .unwrap_or_default();
    EngineOptions {
        device_id: settings.audio_device_id,
        buffer_size: Some(settings.buffer_size),
        filter: app.state::<PlayerState>().filter(),
    }
}

/// The device the settings point to now: the chosen one when it is there,
/// else the system default.
fn target_device(options: &EngineOptions) -> Option<String> {
    let available = output_device_ids().unwrap_or_default();
    options
        .device_id
        .clone()
        .filter(|id| available.contains(id))
        .or_else(default_device_id)
}

/// The file of a library track.
fn track_path<R: Runtime>(app: &AppHandle<R>, id: TrackId) -> Result<std::path::PathBuf, AppError> {
    let library = app.state::<LibraryState>();
    let detail = lock(library.library()?).track(id)?;
    Ok(detail.path)
}

/// Decodes a library track.
fn decode_track<R: Runtime>(app: &AppHandle<R>, id: TrackId) -> Result<AudioBuffer, AppError> {
    Ok(decode_file(&track_path(app, id)?)?)
}

/// Puts track `id` on `engine` (decoding it unless `audio` is given), at
/// `position_sec`, playing if `playing`.
fn put_on_deck<R: Runtime>(
    app: &AppHandle<R>,
    engine: &Engine,
    id: TrackId,
    audio: Option<AudioBuffer>,
    position_sec: f64,
    playing: bool,
) -> Result<(), AppError> {
    let audio = match audio {
        Some(audio) => audio,
        None => decode_track(app, id)?,
    };
    send_load(app, engine, prepare_track(audio, engine.sample_rate())?)?;
    apply_mix(app, engine)?;
    if position_sec > 0.0 {
        engine.seek(DeckId::A, position_sec)?;
    }
    if playing {
        engine.play(DeckId::A)?;
    }
    Ok(())
}

/// Sends a prepared track to the deck. The engine then holds no stems (the
/// old track's leave with it), so this is the one place that records that.
fn send_load<R: Runtime>(
    app: &AppHandle<R>,
    engine: &Engine,
    track: Track,
) -> Result<(), AppError> {
    engine.load(DeckId::A, track)?;
    *lock(&app.state::<PlayerState>().stems_on_deck) = None;
    Ok(())
}

/// Gives `engine` the stem mixer's settings and mode. The stems themselves
/// are read and attached afterwards, outside the gate ([`refresh_stems`]),
/// and are optional: a track without them plays itself.
fn apply_mix<R: Runtime>(app: &AppHandle<R>, engine: &Engine) -> Result<(), AppError> {
    let mix = *lock(&app.state::<PlayerState>().mix);
    for (index, stem) in Stem::ALL.into_iter().enumerate() {
        engine.set_stem_gain(DeckId::A, stem, mix.gain.get(index).copied().unwrap_or(1.0))?;
        engine.set_stem_mute(
            DeckId::A,
            stem,
            mix.muted.get(index).copied().unwrap_or(false),
        )?;
        engine.set_stem_solo(
            DeckId::A,
            stem,
            mix.soloed.get(index).copied().unwrap_or(false),
        )?;
    }
    engine.set_mode(DeckId::A, mix.mode)?;
    Ok(())
}

/// Track `id`'s cached stems prepared at `rate`, or `None` when it was not
/// split. Slow (it reads and resamples about 170 MB for 4 minutes): never
/// called with the gate held.
fn prepared_stems<R: Runtime>(
    app: &AppHandle<R>,
    id: TrackId,
    rate: u32,
) -> Result<Option<StemSet>, AppError> {
    let stored = {
        let state = app.state::<LibraryState>();
        let library = state.library()?;
        lock(library).stems(id)?
    };
    let Some(stored) = stored else {
        return Ok(None);
    };
    Ok(Some(prepare_stems(read_folder(&stored.dir)?, rate)?))
}

/// In stems mode, gives the track on the deck its stems when the engine does
/// not hold them yet. They are read and prepared outside the gate (play,
/// pause and seek never wait for it); under the gate it then checks that the
/// deck, the engine and the mode did not change meanwhile before sending.
fn refresh_stems<R: Runtime>(app: &AppHandle<R>) -> Result<(), AppError> {
    let player = app.state::<PlayerState>();
    let (engine, id) = {
        let _gate = lock(&player.load_gate);
        let (Some(engine), Some(id)) = (player.running(), *lock(&player.on_deck)) else {
            return Ok(());
        };
        if lock(&player.mix).mode != PlaybackMode::Stems || *lock(&player.stems_on_deck) == Some(id)
        {
            return Ok(());
        }
        (engine, id)
    };
    let Some(stems) = prepared_stems(app, id, engine.sample_rate())? else {
        return Ok(());
    };
    let _gate = lock(&player.load_gate);
    let unchanged = player
        .running()
        .is_some_and(|now| Arc::ptr_eq(&now, &engine))
        && *lock(&player.on_deck) == Some(id)
        && *lock(&player.stems_on_deck) != Some(id)
        && lock(&player.mix).mode == PlaybackMode::Stems;
    if unchanged {
        engine.attach_stems(DeckId::A, stems)?;
        *lock(&player.stems_on_deck) = Some(id);
        engine.set_mode(DeckId::A, PlaybackMode::Stems)?;
    }
    Ok(())
}

/// After a stems job: the stems held for a re-split track are out of date,
/// and in stems mode the track on the deck gets the new ones.
pub(crate) fn stems_split_done<R: Runtime>(app: &AppHandle<R>, track_ids: &[TrackId]) {
    let player = app.state::<PlayerState>();
    {
        let _gate = lock(&player.load_gate);
        let deck = *lock(&player.on_deck);
        if !deck.is_some_and(|id| track_ids.contains(&id)) {
            return;
        }
        *lock(&player.stems_on_deck) = None;
    }
    // Stems are optional: on any failure the track keeps playing itself.
    refresh_stems(app).ok();
}

/// What goes back on the deck after a restart.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reload {
    id: TrackId,
    position_sec: f64,
    playing: bool,
}

/// The track to put back after a restart: the one asked for last, else the
/// one on the deck. Position and playing carry over only when it is the
/// track the engine really held; a newer track still loading starts at 0,
/// stopped, as a normal load would.
fn carry_over(
    on_deck: Option<TrackId>,
    requested: Option<TrackId>,
    position_sec: f64,
    playing: bool,
) -> Option<Reload> {
    let id = requested.or(on_deck)?;
    Some(if on_deck == Some(id) {
        Reload {
            id,
            position_sec,
            playing,
        }
    } else {
        Reload {
            id,
            position_sec: 0.0,
            playing: false,
        }
    })
}

/// How a restart was asked for.
struct Restart {
    /// Only restart this engine: when another restart replaced it meanwhile,
    /// there is nothing left to do (the poller's device check).
    expected: Option<Arc<Engine>>,
    /// Skip when the engine already uses the device and buffer size the
    /// settings point to (after a settings change).
    skip_if_same: bool,
}

/// Restarts a running engine on the device the settings choose now (or the
/// system default) and puts the track back. Returns the id of the device it
/// opened, or `None` when there was nothing to restart.
///
/// The old engine is closed before the new one opens (some systems give a
/// device to one stream at a time). The track is decoded first, before the
/// gate, so play, pause and seek wait as little as possible.
fn restart<R: Runtime>(app: &AppHandle<R>, how: &Restart) -> Result<Option<String>, AppError> {
    let player = app.state::<PlayerState>();
    let wanted = (*lock(&player.requested)).or(*lock(&player.on_deck));
    let decoded = wanted.map(|id| (id, decode_track(app, id)));

    let gate = lock(&player.load_gate);
    let Some(old) = player.running() else {
        return Ok(None);
    };
    if let Some(expected) = &how.expected
        && !Arc::ptr_eq(&old, expected)
    {
        return Ok(None);
    }
    let options = engine_options(app);
    if how.skip_if_same
        && old.buffer_size() == options.buffer_size
        && target_device(&options).as_deref() == Some(old.device_id())
    {
        return Ok(Some(old.device_id().to_owned()));
    }
    let before = old.status(DeckId::A);
    old.close();
    *lock(&player.engine) = None;
    drop(old);

    let engine = match Engine::start(&options) {
        Ok(engine) => Arc::new(engine),
        Err(err) => {
            if before.playing {
                send_playing(app, false);
            }
            return Err(err.into());
        }
    };
    *lock(&player.engine) = Some(Arc::clone(&engine));
    *lock(&player.stems_on_deck) = None;

    let on_deck = *lock(&player.on_deck);
    let requested = *lock(&player.requested);
    let mut playing = false;
    if let Some(reload) = carry_over(on_deck, requested, before.position_sec, before.playing) {
        let audio = match decoded {
            Some((id, Ok(audio))) if id == reload.id => Some(audio),
            _ => None,
        };
        match put_on_deck(
            app,
            &engine,
            reload.id,
            audio,
            reload.position_sec,
            reload.playing,
        ) {
            Ok(()) => {
                *lock(&player.on_deck) = Some(reload.id);
                playing = reload.playing;
            }
            Err(_) => {
                // That track cannot play any more (moved, damaged): the deck
                // is empty, and it is not reloaded again later.
                *lock(&player.on_deck) = None;
                if requested == Some(reload.id) {
                    *lock(&player.requested) = None;
                }
            }
        }
    }
    if before.playing || playing {
        send_playing(app, playing);
    }
    drop(gate);
    // The stems are prepared again for the new engine, outside the gate; on
    // any failure the track plays itself.
    refresh_stems(app).ok();
    Ok(Some(engine.device_id().to_owned()))
}

/// Applies a new output device or buffer size from the settings to a
/// running engine (it restarts and keeps the track), on a blocking thread.
/// Returns the device it uses, or `None` when no engine was running.
pub(crate) async fn apply_audio_settings(app: AppHandle) -> Result<Option<String>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        restart(
            &app,
            &Restart {
                expected: None,
                skip_if_same: true,
            },
        )
    })
    .await
    .map_err(internal)?
}

/// Tells the UI whether the deck plays (after a restart changed it).
fn send_playing<R: Runtime>(app: &AppHandle<R>, playing: bool) {
    // A window that is closing cannot receive it; the others still do.
    app.emit("playback_state", PlaybackState { playing }).ok();
}

fn internal(err: tauri::Error) -> AppError {
    AppError {
        code: "internal",
        message: err.to_string(),
    }
}

/// `playback_position`: the UI's position event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
struct Position {
    sec: f64,
}

/// `meter`: the UI's `MeterReading`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
struct Meter {
    peak_db: f32,
    rms_db: f32,
}

/// `playback_state`: the UI's `PlaybackState`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
struct PlaybackState {
    playing: bool,
}

/// Why the output device changed (the UI's `AudioDeviceChanged.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DeviceChange {
    /// The user picked it (or the picked device came back).
    Selected,
    /// The device in use disappeared; playback moved to the default.
    Unplugged,
    /// The system default changed while the app follows it.
    DefaultChanged,
    /// The output failed and no other one could be opened: nothing plays.
    Lost,
    /// Devices were plugged or unplugged: only the list changed.
    ListChanged,
}

/// `audio_device_changed`: the UI's `AudioDeviceChanged`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub(crate) struct DeviceChanged {
    id: String,
    reason: DeviceChange,
}

/// Sends `audio_device_changed`.
fn announce<R: Runtime>(app: &AppHandle<R>, id: String, reason: DeviceChange) {
    // A window that is closing cannot receive it; the others still do.
    app.emit("audio_device_changed", DeviceChanged { id, reason })
        .ok();
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

/// Whether the engine should move to another device: the chosen one when
/// it is available, else the system default. Returns why, or `None` when
/// it already uses the right one (or there is nowhere to go).
fn device_switch(
    chosen: Option<&str>,
    opened: &str,
    default: Option<&str>,
    available: &[String],
) -> Option<DeviceChange> {
    let chosen = chosen.filter(|id| available.iter().any(|device| device == id));
    let target = chosen.or(default)?;
    if target == opened {
        return None;
    }
    Some(if chosen.is_some() {
        DeviceChange::Selected
    } else {
        DeviceChange::DefaultChanged
    })
}

/// Starts the poller thread, for as long as the app runs: playback events
/// about 30 times a second, and the device check about every 2 s.
pub(crate) fn start_poller<R: Runtime>(app: AppHandle<R>) {
    thread::spawn(move || {
        let mut previous: Option<Status> = None;
        let mut devices: Option<(Vec<String>, Option<String>)> = None;
        let mut polls = 0_u32;
        loop {
            thread::sleep(POLL_INTERVAL);
            polls = polls.wrapping_add(1);
            match app.state::<PlayerState>().running() {
                Some(engine) => {
                    let status = engine.status(DeckId::A);
                    if status.device_error {
                        // The output went away or broke: move to the default.
                        let lost = engine.device_id().to_owned();
                        drop(engine);
                        let how = Restart {
                            expected: None,
                            skip_if_same: false,
                        };
                        match restart(&app, &how) {
                            Ok(Some(id)) => announce(&app, id, DeviceChange::Unplugged),
                            Ok(None) => {}
                            Err(_) => announce(&app, lost, DeviceChange::Lost),
                        }
                        previous = None;
                        continue;
                    }
                    send_playback_events(&app, previous.as_ref(), &status);
                    previous = Some(status);
                }
                None => previous = None,
            }
            if polls.is_multiple_of(DEVICE_CHECK_POLLS) {
                check_devices(&app, &mut devices);
            }
        }
    });
}

/// Sends position, meter and end-of-track events for `status`.
fn send_playback_events<R: Runtime>(
    app: &AppHandle<R>,
    previous: Option<&Status>,
    status: &Status,
) {
    let (position, meter, ended) = events(previous, status);
    // A window that is closing cannot receive events: nothing to do.
    if let Some(position) = position {
        app.emit("playback_position", position).ok();
    }
    if let Some(meter) = meter {
        app.emit("meter", meter).ok();
    }
    if ended {
        send_playing(app, false);
    }
}

/// The device check: tells the UI when the device list changed, moves a
/// running engine to the chosen device when it comes back or to a new
/// system default, and brings a deck back after a failed restart.
fn check_devices<R: Runtime>(app: &AppHandle<R>, seen: &mut Option<(Vec<String>, Option<String>)>) {
    // A listing that fails is skipped, never read as "no devices".
    let Ok(available) = output_device_ids() else {
        return;
    };
    let default = default_device_id();
    let now = (available, default);
    if seen.as_ref().is_some_and(|before| *before != now) {
        announce(
            app,
            now.1.clone().unwrap_or_default(),
            DeviceChange::ListChanged,
        );
    }
    *seen = Some(now.clone());
    let (available, default) = now;

    let player = app.state::<PlayerState>();
    let chosen = engine_options(app).device_id;
    match player.running() {
        Some(engine) => {
            let opened = engine.device_id().to_owned();
            let Some(reason) =
                device_switch(chosen.as_deref(), &opened, default.as_deref(), &available)
            else {
                return;
            };
            let how = Restart {
                expected: Some(engine),
                skip_if_same: false,
            };
            match restart(app, &how) {
                Ok(Some(id)) => announce(app, id, reason),
                Ok(None) => {}
                Err(_) => announce(app, opened, DeviceChange::Lost),
            }
        }
        // A restart failed earlier and the deck had a track: try again now
        // that a device may be back.
        None if lock(&player.on_deck).is_some() && !available.is_empty() => {
            let _gate = lock(&player.load_gate);
            if let Ok(engine) = player.engine_locked(app) {
                let reason = if chosen.as_deref() == Some(engine.device_id()) {
                    DeviceChange::Selected
                } else {
                    DeviceChange::DefaultChanged
                };
                announce(app, engine.device_id().to_owned(), reason);
            }
        }
        None => {}
    }
}

/// Loads a track from the library: decodes it, resamples it to the device
/// and puts it on the deck, stopped at the start. When another load is asked
/// for meanwhile, this one stops without touching the deck (the UI ignores
/// its stale answer too).
#[tauri::command]
pub(crate) async fn playback_load(app: AppHandle, id: TrackId) -> Result<(), AppError> {
    let player = app.state::<PlayerState>();
    let generation = player.loads.fetch_add(1, Ordering::SeqCst) + 1;
    // A restart from now on puts this track on the deck.
    *lock(&player.requested) = Some(id);
    tauri::async_runtime::spawn_blocking(move || {
        let player = app.state::<PlayerState>();
        let current = || player.loads.load(Ordering::SeqCst) == generation;
        let result = (|| {
            let audio = decode_track(&app, id)?;
            if !current() {
                return Ok(());
            }
            // Only the rate is needed to prepare the track: no engine is held
            // while preparing, so a restart can close it.
            let rate = {
                let _gate = lock(&player.load_gate);
                player.engine_locked(&app)?.sample_rate()
            };
            let track = prepare_track(audio, rate)?;
            let _gate = lock(&player.load_gate);
            if !current() {
                return Ok(());
            }
            let engine = player.engine_locked(&app)?;
            // A restart moved to another rate meanwhile, and already put
            // this track on the new engine.
            if engine.sample_rate() != rate {
                return Ok(());
            }
            send_load(&app, &engine, track)?;
            *lock(&player.on_deck) = Some(id);
            // Every load starts with a neutral stem mix (the UI does the
            // same); the mode carries over.
            {
                let mut mix = lock(&player.mix);
                *mix = StemMix {
                    mode: mix.mode,
                    ..StemMix::default()
                };
            }
            apply_mix(&app, &engine)?;
            Ok(())
        })();
        if result.is_ok() && current() {
            // Stems are optional: on any failure the track plays itself.
            refresh_stems(&app).ok();
        }
        if result.is_err() && current() {
            // A track that failed is not reloaded by a later restart.
            let deck = *lock(&player.on_deck);
            *lock(&player.requested) = deck;
        }
        result
    })
    .await
    .map_err(internal)?
}

/// Runs `work` on the engine on a blocking thread, after any restart in
/// progress (it waits for `load_gate`). `start` opens the engine when there
/// is none; otherwise nothing happens without one.
async fn with_engine<F>(app: AppHandle, start: bool, work: F) -> Result<(), AppError>
where
    F: FnOnce(&Engine) -> Result<(), EngineError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let player = app.state::<PlayerState>();
        let _gate = lock(&player.load_gate);
        let engine = if start {
            Some(player.engine_locked(&app)?)
        } else {
            player.running()
        };
        if let Some(engine) = engine {
            work(&engine)?;
        }
        Ok(())
    })
    .await
    .map_err(internal)?
}

/// Plays the loaded track.
#[tauri::command]
pub(crate) async fn playback_play(app: AppHandle) -> Result<(), AppError> {
    with_engine(app, true, |engine| engine.play(DeckId::A)).await
}

/// Pauses.
#[tauri::command]
pub(crate) async fn playback_pause(app: AppHandle) -> Result<(), AppError> {
    with_engine(app, false, |engine| engine.pause(DeckId::A)).await
}

/// Jumps to `sec` from the start.
#[tauri::command]
pub(crate) async fn playback_seek(app: AppHandle, sec: f64) -> Result<(), AppError> {
    with_engine(app, false, move |engine| engine.seek(DeckId::A, sec)).await
}

/// Turns the DJ filter knob: -1 full low-pass, 0 off, 1 full high-pass
/// (clamped; the engine smooths the change). Kept for engines started later.
#[tauri::command]
pub(crate) async fn playback_set_filter(app: AppHandle, value: f32) -> Result<(), AppError> {
    let value = if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    // Stored first, so an engine that starts meanwhile starts there. The
    // engine then gets the stored value read under the gate, not `value`:
    // when several calls wait for a restart, whichever runs last still
    // sends the newest knob position.
    *lock(&app.state::<PlayerState>().filter) = value;
    let reader = app.clone();
    with_engine(app, false, move |engine| {
        engine.set_filter(DeckId::A, reader.state::<PlayerState>().filter())
    })
    .await
}

/// What the UI asks the deck to play (the UI's `PlaybackMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub(crate) enum ModeChoice {
    /// The track itself.
    Original,
    /// The mastered track (#26).
    Mastered,
    /// The track's stems.
    Stems,
}

/// A stem as the UI names it (the UI's `StemName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub(crate) enum StemName {
    /// Vocals.
    Vocals,
    /// Drums.
    Drums,
    /// Bass.
    Bass,
    /// Other.
    Other,
}

impl StemName {
    fn stem(self) -> Stem {
        match self {
            Self::Vocals => Stem::Vocals,
            Self::Drums => Stem::Drums,
            Self::Bass => Stem::Bass,
            Self::Other => Stem::Other,
        }
    }

    /// Its index in [`Stem::ALL`] (and in [`StemMix`]).
    fn index(self) -> usize {
        Stem::ALL
            .iter()
            .position(|&stem| stem == self.stem())
            .unwrap_or(0)
    }
}

/// Plays the track or its stems (crossfaded). In stems mode the track's
/// cached stems are read and attached (outside the gate); a track that was
/// not split plays itself. The mastered mode comes with mastering (#26).
#[tauri::command]
pub(crate) async fn playback_set_mode(app: AppHandle, mode: ModeChoice) -> Result<(), AppError> {
    let mode = match mode {
        ModeChoice::Original => PlaybackMode::Original,
        ModeChoice::Stems => PlaybackMode::Stems,
        ModeChoice::Mastered => {
            return Err(AppError {
                code: "not_built",
                message: "mastered playback".into(),
            });
        }
    };
    let previous = std::mem::replace(&mut lock(&app.state::<PlayerState>().mix).mode, mode);
    tauri::async_runtime::spawn_blocking(move || {
        let player = app.state::<PlayerState>();
        let send_stored = || -> Result<(), AppError> {
            let _gate = lock(&player.load_gate);
            // The stored mode, read under the gate: when several calls wait,
            // the engine still ends on the newest one.
            if let Some(engine) = player.running() {
                engine.set_mode(DeckId::A, lock(&player.mix).mode)?;
            }
            Ok(())
        };
        let switched = send_stored().and_then(|()| refresh_stems(&app));
        if switched.is_err() {
            // Back to what the deck played before, here and in the engine.
            let mut mix = lock(&player.mix);
            if mix.mode == mode {
                mix.mode = previous;
            }
            drop(mix);
            send_stored().ok();
        }
        switched
    })
    .await
    .map_err(internal)?
}

/// A stem's gain (0 to 2; clamped), kept for later tracks and engines.
#[tauri::command]
pub(crate) async fn stems_set_gain(
    app: AppHandle,
    stem: StemName,
    gain: f32,
) -> Result<(), AppError> {
    let gain = if gain.is_finite() {
        gain.clamp(0.0, 2.0)
    } else {
        1.0
    };
    if let Some(slot) = lock(&app.state::<PlayerState>().mix)
        .gain
        .get_mut(stem.index())
    {
        *slot = gain;
    }
    // The stored value, read under the gate, so the newest one wins.
    let reader = app.clone();
    with_engine(app, false, move |engine| {
        let value = lock(&reader.state::<PlayerState>().mix)
            .gain
            .get(stem.index())
            .copied()
            .unwrap_or(1.0);
        engine.set_stem_gain(DeckId::A, stem.stem(), value)
    })
    .await
}

/// Mutes or unmutes a stem, kept for later tracks and engines.
#[tauri::command]
pub(crate) async fn stems_set_mute(
    app: AppHandle,
    stem: StemName,
    muted: bool,
) -> Result<(), AppError> {
    if let Some(slot) = lock(&app.state::<PlayerState>().mix)
        .muted
        .get_mut(stem.index())
    {
        *slot = muted;
    }
    // The stored value, read under the gate, so the newest one wins.
    let reader = app.clone();
    with_engine(app, false, move |engine| {
        let value = lock(&reader.state::<PlayerState>().mix)
            .muted
            .get(stem.index())
            .copied()
            .unwrap_or(false);
        engine.set_stem_mute(DeckId::A, stem.stem(), value)
    })
    .await
}

/// Solos or unsolos a stem (while any is soloed, exactly the soloed ones are
/// heard), kept for later tracks and engines.
#[tauri::command]
pub(crate) async fn stems_set_solo(
    app: AppHandle,
    stem: StemName,
    soloed: bool,
) -> Result<(), AppError> {
    if let Some(slot) = lock(&app.state::<PlayerState>().mix)
        .soloed
        .get_mut(stem.index())
    {
        *slot = soloed;
    }
    // The stored value, read under the gate, so the newest one wins.
    let reader = app.clone();
    with_engine(app, false, move |engine| {
        let value = lock(&reader.state::<PlayerState>().mix)
            .soloed
            .get(stem.index())
            .copied()
            .unwrap_or(false);
        engine.set_stem_solo(DeckId::A, stem.stem(), value)
    })
    .await
}

/// An output device as the UI's `AudioDevice`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioDeviceDto {
    id: String,
    name: String,
    is_default: bool,
    sample_rate: u32,
}

impl From<OutputDevice> for AudioDeviceDto {
    fn from(device: OutputDevice) -> Self {
        Self {
            id: device.id,
            name: device.name,
            is_default: device.is_default,
            sample_rate: device.sample_rate,
        }
    }
}

/// Every output device, the system default marked.
#[tauri::command]
pub(crate) async fn audio_list_devices() -> Result<Vec<AudioDeviceDto>, AppError> {
    let devices = tauri::async_runtime::spawn_blocking(output_devices)
        .await
        .map_err(internal)??;
    Ok(devices.into_iter().map(AudioDeviceDto::from).collect())
}

/// Chooses the output device: stores it in the settings (so it is used
/// again after a restart of the app), moves playback to it, and tells every
/// window (`settings_changed`, then `audio_device_changed` with the device
/// actually opened). Following the system default again is the setting
/// `audioDeviceId` set to null.
#[tauri::command]
pub(crate) async fn audio_set_device(app: AppHandle, id: String) -> Result<(), AppError> {
    let value = serde_json::Value::String(id.clone());
    let settings = crate::library::with_library(app.clone(), move |library| {
        library.set_setting("audioDeviceId", value)
    })
    .await?;
    app.emit("settings_changed", settings).ok();
    report_applied(&app, apply_audio_settings(app.clone()).await, id);
    Ok(())
}

/// Tells the UI what applying an audio setting did. The setting is saved
/// either way, so a failure here is a notice, not an error of the command.
pub(crate) fn report_applied<R: Runtime>(
    app: &AppHandle<R>,
    applied: Result<Option<String>, AppError>,
    wanted: String,
) {
    match applied {
        Ok(Some(opened)) => announce(app, opened, DeviceChange::Selected),
        Ok(None) => {}
        Err(_) => announce(app, wanted, DeviceChange::Lost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_and_stems_deserialize_from_the_ui_names() {
        let mode: ModeChoice = serde_json::from_value(serde_json::json!("stems")).unwrap();
        assert_eq!(mode, ModeChoice::Stems);
        let mode: ModeChoice = serde_json::from_value(serde_json::json!("mastered")).unwrap();
        assert_eq!(mode, ModeChoice::Mastered);
        let stem: StemName = serde_json::from_value(serde_json::json!("vocals")).unwrap();
        assert_eq!(stem, StemName::Vocals);
        assert!(serde_json::from_value::<StemName>(serde_json::json!("guitar")).is_err());
    }

    #[test]
    fn each_stem_name_has_its_engine_stem_and_slot() {
        let names = [
            StemName::Drums,
            StemName::Bass,
            StemName::Other,
            StemName::Vocals,
        ];
        for (index, name) in names.into_iter().enumerate() {
            assert_eq!(name.index(), index);
            assert_eq!(Stem::ALL[index], name.stem());
        }
    }

    #[test]
    fn the_stem_mixer_starts_at_unity_on_the_track() {
        let mix = StemMix::default();
        assert_eq!(mix.mode, PlaybackMode::Original);
        assert_eq!(mix.gain, [1.0; 4]);
        assert_eq!(mix.muted, [false; 4]);
        assert_eq!(mix.soloed, [false; 4]);
    }

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

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn a_restart_keeps_the_place_only_for_the_track_really_on_the_deck() {
        // The same track: position and playing carry over.
        assert_eq!(
            carry_over(Some(7), Some(7), 42.5, true),
            Some(Reload {
                id: 7,
                position_sec: 42.5,
                playing: true
            })
        );
        // A newer track was still loading: it starts at 0, stopped.
        assert_eq!(
            carry_over(Some(7), Some(9), 42.5, true),
            Some(Reload {
                id: 9,
                position_sec: 0.0,
                playing: false
            })
        );
        // Nothing asked for after the deck's track (or it failed and was
        // cleared): the deck's track.
        assert_eq!(
            carry_over(Some(7), None, 3.0, false),
            Some(Reload {
                id: 7,
                position_sec: 3.0,
                playing: false
            })
        );
        // First load still decoding, nothing on the deck yet.
        assert_eq!(
            carry_over(None, Some(9), 0.0, false),
            Some(Reload {
                id: 9,
                position_sec: 0.0,
                playing: false
            })
        );
        assert_eq!(carry_over(None, None, 10.0, true), None);
    }

    #[test]
    fn follows_the_chosen_device_else_the_default() {
        let available = ids(&["speakers", "usb"]);
        // Already right.
        assert_eq!(
            device_switch(Some("usb"), "usb", Some("speakers"), &available),
            None
        );
        assert_eq!(
            device_switch(None, "speakers", Some("speakers"), &available),
            None
        );
        // The chosen device came back after a fallback.
        assert_eq!(
            device_switch(Some("usb"), "speakers", Some("speakers"), &available),
            Some(DeviceChange::Selected)
        );
        // Following the default, and it changed.
        assert_eq!(
            device_switch(None, "speakers", Some("usb"), &available),
            Some(DeviceChange::DefaultChanged)
        );
        // The chosen device is gone: the default it fell back to changed.
        assert_eq!(
            device_switch(Some("gone"), "speakers", Some("usb"), &available),
            Some(DeviceChange::DefaultChanged)
        );
        assert_eq!(
            device_switch(Some("gone"), "usb", Some("usb"), &available),
            None
        );
        // No default and nothing chosen: nowhere to go.
        assert_eq!(device_switch(None, "usb", None, &available), None);
    }

    #[test]
    fn device_payloads_serialize_as_the_ui_types() {
        let device = AudioDeviceDto::from(OutputDevice {
            id: "core:usb".into(),
            name: "USB Audio".into(),
            is_default: false,
            sample_rate: 48_000,
        });
        assert_eq!(
            serde_json::to_value(device).unwrap(),
            serde_json::json!({ "id": "core:usb", "name": "USB Audio", "isDefault": false, "sampleRate": 48000 })
        );
        for (reason, text) in [
            (DeviceChange::Selected, "selected"),
            (DeviceChange::Unplugged, "unplugged"),
            (DeviceChange::DefaultChanged, "default-changed"),
            (DeviceChange::Lost, "lost"),
            (DeviceChange::ListChanged, "list-changed"),
        ] {
            assert_eq!(
                serde_json::to_value(DeviceChanged {
                    id: "x".into(),
                    reason
                })
                .unwrap(),
                serde_json::json!({ "id": "x", "reason": text })
            );
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
