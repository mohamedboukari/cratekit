//! Library commands: thin wrappers over `cratekit_core::library`.
//!
//! The library lives in `cratekit.db` in the app data folder. One `Library`
//! (one SQLite connection, the single writer) sits behind a lock; commands
//! run on a blocking worker thread so a big folder scan never freezes the
//! window.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use cratekit_core::CoreError;
use cratekit_core::analysis::waveform::WaveformView;
use cratekit_core::library::{
    Analysis, Crate, Cue, CueColor, Library, LibraryCounts, LibraryQuery, TrackId, TrackRow,
    Transition, prepare_add,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::AppError;

/// The open library, or why it could not be opened (every command then
/// returns that error, so the UI can say so).
pub(crate) struct LibraryState(Result<Mutex<Library>, AppError>);

impl LibraryState {
    /// Opens `cratekit.db` in the app data folder, creating both if needed.
    pub(crate) fn open<R: Runtime>(app: &AppHandle<R>) -> Self {
        let opened = app
            .path()
            .app_data_dir()
            .map_err(AppError::from)
            .and_then(|dir| {
                std::fs::create_dir_all(&dir)
                    .map_err(|err| AppError::from(CoreError::from(err)))?;
                Library::open(&dir.join("cratekit.db")).map_err(AppError::from)
            });
        Self(opened.map(Mutex::new))
    }

    /// The library, or why it could not be opened.
    pub(crate) fn library(&self) -> Result<&Mutex<Library>, AppError> {
        self.0.as_ref().map_err(Clone::clone)
    }
}

/// Runs `work` on the library on a blocking worker thread.
pub(crate) async fn with_library<T, F>(app: AppHandle, work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&mut Library) -> Result<T, CoreError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<LibraryState>();
        let library = state.library()?;
        // A poisoned lock still holds a usable connection: SQLite rolled back
        // any transaction the panicking thread had open.
        let mut library = library.lock().unwrap_or_else(PoisonError::into_inner);
        work(&mut library).map_err(AppError::from)
    })
    .await
    .map_err(internal)?
}

/// A worker thread that failed to finish (it panicked).
fn internal(err: tauri::Error) -> AppError {
    AppError {
        code: "internal",
        message: err.to_string(),
    }
}

/// A browser row as the UI's `TrackRow`: the core row plus the live job
/// activity, which the job queue (#15) will fill in.
#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub(crate) struct TrackRowDto {
    #[serde(flatten)]
    row: TrackRow,
    activity: Option<()>,
}

impl From<TrackRow> for TrackRowDto {
    fn from(row: TrackRow) -> Self {
        Self {
            row,
            activity: None,
        }
    }
}

/// One track as the UI's `TrackDetail`, with the waveform overview when the
/// analysis stored one.
#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub(crate) struct TrackDetailDto {
    track: TrackRowDto,
    analysis: Option<Analysis>,
    cues: Vec<Cue>,
    waveform: Option<WaveformView>,
}

/// Adds files and folders (searched with their subfolders); returns the new
/// track ids. Files that cannot be read are skipped; when none could be
/// added and some failed, the first failure is the error, so the user sees
/// why instead of "no new files".
///
/// Reading the files (the slow part) runs without the library lock, so the
/// browser keeps answering during a big scan; the lock is held only to read
/// the known paths and to store the result.
#[tauri::command]
pub(crate) async fn library_add_paths(
    app: AppHandle,
    paths: Vec<PathBuf>,
) -> Result<Vec<TrackId>, AppError> {
    let known = with_library(app.clone(), |library| library.known_paths()).await?;
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_add(&paths, &known))
        .await
        .map_err(internal)?;
    let result = with_library(app, move |library| library.store(prepared)).await?;
    if result.added.is_empty()
        && let Some((_, first_failure)) = result.failed.into_iter().next()
    {
        return Err(first_failure.into());
    }
    Ok(result.added)
}

/// The tracks matching the browser's filters.
#[tauri::command]
pub(crate) async fn library_list(
    app: AppHandle,
    query: LibraryQuery,
) -> Result<Vec<TrackRowDto>, AppError> {
    with_library(app, move |library| {
        library
            .list(&query)
            .map(|rows| rows.into_iter().map(TrackRowDto::from).collect())
    })
    .await
}

/// The sidebar counts.
#[tauri::command]
pub(crate) async fn library_counts(app: AppHandle) -> Result<LibraryCounts, AppError> {
    with_library(app, |library| library.counts()).await
}

/// How each pair of neighbours in the set list fits (key, BPM change).
#[tauri::command]
pub(crate) async fn setlist_transitions(
    app: AppHandle,
    ids: Vec<TrackId>,
) -> Result<Vec<Transition>, AppError> {
    with_library(app, move |library| library.transitions(&ids)).await
}

/// Every crate with its track count.
#[tauri::command]
pub(crate) async fn crates_list(app: AppHandle) -> Result<Vec<Crate>, AppError> {
    with_library(app, |library| library.crates()).await
}

/// Makes a crate holding `ids` in that order, in one step (the set
/// builder's "Save as crate"; `ids` empty for an empty crate).
#[tauri::command]
pub(crate) async fn crate_create(
    app: AppHandle,
    name: String,
    color: CueColor,
    ids: Vec<TrackId>,
) -> Result<Crate, AppError> {
    with_library(app, move |library| library.crate_create(&name, color, &ids)).await
}

/// Adds tracks to the end of a crate, in the order given.
#[tauri::command]
pub(crate) async fn crate_add_tracks(
    app: AppHandle,
    crate_id: i64,
    ids: Vec<TrackId>,
) -> Result<(), AppError> {
    with_library(app, move |library| library.crate_add_tracks(crate_id, &ids)).await
}

/// Takes tracks out of a crate (they stay in the library).
#[tauri::command]
pub(crate) async fn crate_remove_tracks(
    app: AppHandle,
    crate_id: i64,
    ids: Vec<TrackId>,
) -> Result<(), AppError> {
    with_library(app, move |library| {
        library.crate_remove_tracks(crate_id, &ids)
    })
    .await
}

/// One track with its cues.
#[tauri::command]
pub(crate) async fn track_detail(app: AppHandle, id: TrackId) -> Result<TrackDetailDto, AppError> {
    with_library(app, move |library| {
        library.track(id).map(|detail| TrackDetailDto {
            track: detail.track.into(),
            analysis: detail.analysis,
            cues: detail.cues,
            waveform: detail.waveform,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use cratekit_core::library::{CueColor, TrackRow};

    use super::*;

    fn row() -> TrackRow {
        TrackRow {
            id: 7,
            title: "Test Tone".into(),
            artist: "Cratekit".into(),
            duration_sec: 0.5,
            bpm: Some(124.0),
            camelot: Some("8A".into()),
            key_name: Some("A minor".into()),
            energy: Some(7),
            lufs: Some(-9.5),
            analyzed: true,
            has_stems: false,
            mastered: false,
        }
    }

    #[test]
    fn rows_serialize_as_the_ui_track_row() {
        let json = serde_json::to_value(TrackRowDto::from(row())).expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "id": 7, "title": "Test Tone", "artist": "Cratekit", "durationSec": 0.5,
                "bpm": 124.0, "camelot": "8A", "keyName": "A minor", "energy": 7,
                "lufs": -9.5, "analyzed": true, "hasStems": false, "mastered": false,
                "activity": null
            })
        );
    }

    #[test]
    fn details_serialize_as_the_ui_track_detail() {
        let detail = TrackDetailDto {
            track: row().into(),
            analysis: None,
            cues: vec![Cue {
                slot: 3,
                position_sec: 12.5,
                color: CueColor::Blue,
                name: Some("Drop".into()),
            }],
            waveform: None,
        };
        let json = serde_json::to_value(detail).expect("serialize");
        assert_eq!(json["analysis"], serde_json::Value::Null);
        assert_eq!(json["waveform"], serde_json::Value::Null);
        assert_eq!(
            json["cues"],
            serde_json::json!([{ "slot": 3, "positionSec": 12.5, "color": "blue", "name": "Drop" }])
        );
        assert_eq!(json["track"]["activity"], serde_json::Value::Null);
    }

    #[test]
    fn queries_deserialize_from_the_ui_library_query() {
        let query: LibraryQuery = serde_json::from_value(serde_json::json!({
            "crateId": null, "search": "tone", "bpmMin": 120, "bpmMax": null,
            "nearBpm": null, "matchCamelot": "8A", "hasStems": false
        }))
        .expect("deserialize");
        assert_eq!(query.search, "tone");
        assert_eq!(query.bpm_min, Some(120.0));
        assert_eq!(query.match_camelot.as_deref(), Some("8A"));
    }

    #[test]
    fn crates_match_the_ui_crate_and_colour_names() {
        let color: CueColor = serde_json::from_value(serde_json::json!("lime")).expect("colour");
        assert_eq!(color, CueColor::Lime);
        let json = serde_json::to_value(Crate {
            id: 4,
            name: "Friday set".into(),
            color,
            track_count: 12,
        })
        .expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({ "id": 4, "name": "Friday set", "color": "lime", "trackCount": 12 })
        );
    }
}
