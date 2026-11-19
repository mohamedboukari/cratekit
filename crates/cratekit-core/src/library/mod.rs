//! The track library (SPEC, Module 1): one SQLite file with the user's
//! tracks, their analysis, cues, crates and settings. See docs/library.md.
//!
//! - **Never touches the music.** Adding reads a file's tags and headers;
//!   removing a track deletes its row, never the file.
//! - **One writer.** A [`Library`] owns one connection; the app keeps one
//!   behind a lock, so all writes go through it. WAL mode lets other
//!   connections read while it writes.

mod scan;
mod schema;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::thread;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use scan::{AUDIO_EXTENSIONS, has_audio_extension};

use crate::CoreError;
use crate::analysis::ANALYZER_VERSION;
use crate::analysis::key::{Camelot, Key, Mode, PitchClass};
use crate::analysis::track::TrackAnalysis;
use crate::audio::{FileInfo, probe_file};
use crate::settings::Settings;

/// A track's id in the library.
pub type TrackId = i64;

/// "Near" a BPM, for the deck's near-BPM filter: within this many BPM.
pub const NEAR_BPM_TOLERANCE: f64 = 3.0;

/// A BPM change between neighbours in the set list above this many percent
/// is too big to mix smoothly (a "jump").
pub const MAX_BPM_CHANGE_PCT: f64 = 6.0;

/// How long a track counts as recently added.
const RECENT_DAYS: u32 = 7;

/// Most files probed at the same time when adding.
const MAX_PROBE_THREADS: usize = 8;

/// One row of the track browser (the UI's `TrackRow`, without the live job
/// activity, which the app adds).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    /// The track's id.
    pub id: TrackId,
    /// The title tag, or the file name without its extension.
    pub title: String,
    /// The artist tag, or empty.
    pub artist: String,
    /// Length in seconds (0 until known).
    pub duration_sec: f64,
    /// Tempo, once analyzed.
    pub bpm: Option<f64>,
    /// Camelot code such as "8A", once analyzed.
    pub camelot: Option<String>,
    /// Key name such as "A minor", once analyzed.
    pub key_name: Option<String>,
    /// Energy 1 to 10, once analyzed.
    pub energy: Option<u8>,
    /// Integrated loudness in LUFS, once analyzed.
    pub lufs: Option<f64>,
    /// Whether an analysis is stored.
    pub analyzed: bool,
    /// Whether stems are stored.
    pub has_stems: bool,
    /// Whether a master was exported.
    pub mastered: bool,
}

/// The browser's filters (the UI's `LibraryQuery`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryQuery {
    /// Only tracks in this crate.
    pub crate_id: Option<i64>,
    /// Words that must appear in the title, artist or album (any case).
    pub search: String,
    /// Lowest BPM.
    pub bpm_min: Option<f64>,
    /// Highest BPM.
    pub bpm_max: Option<f64>,
    /// Only tracks within [`NEAR_BPM_TOLERANCE`] of this BPM.
    pub near_bpm: Option<f64>,
    /// Only tracks whose key mixes well with this Camelot code.
    pub match_camelot: Option<String>,
    /// Only tracks with stems.
    pub has_stems: bool,
}

/// The sidebar's counts (the UI's `LibraryCounts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCounts {
    /// Every track.
    pub all: u32,
    /// Tracks added in the last 7 days.
    pub recently_added: u32,
    /// Tracks with no analysis, or one from an older analyzer.
    pub needs_analysis: u32,
}

/// Hot cue colours (the UI's `CueColor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CueColor {
    /// Orange, the default.
    Orange,
    /// Blue.
    Blue,
    /// Pink.
    Pink,
    /// Purple.
    Purple,
    /// Lime.
    Lime,
    /// Yellow.
    Yellow,
    /// Red.
    Red,
    /// Teal.
    Teal,
}

impl CueColor {
    /// The colour stored as `name`, or orange for anything else.
    fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("blue") => Self::Blue,
            Some("pink") => Self::Pink,
            Some("purple") => Self::Purple,
            Some("lime") => Self::Lime,
            Some("yellow") => Self::Yellow,
            Some("red") => Self::Red,
            Some("teal") => Self::Teal,
            _ => Self::Orange,
        }
    }
}

/// A hot cue (the UI's `Cue`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    /// Pad 1 to 8.
    pub slot: u8,
    /// Position in seconds.
    pub position_sec: f64,
    /// Colour.
    pub color: CueColor,
    /// Optional name.
    pub name: Option<String>,
}

/// A stored analysis as the Analyze tab shows it (the UI's `Analysis`).
/// Everything derived (key name, Camelot code, scale, mixing partners) is
/// computed here, not in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    /// Tempo, or `None` for a track with no steady beat.
    pub bpm: Option<f64>,
    /// 0 to 1 (0 without a tempo).
    pub bpm_confidence: f64,
    /// Key name such as "A minor", or `None` without notes.
    pub key_name: Option<String>,
    /// Camelot code such as "8A".
    pub camelot: Option<String>,
    /// 0 to 1 (0 without a key).
    pub key_confidence: f64,
    /// Energy 1 to 10.
    pub energy: Option<u8>,
    /// Integrated loudness in LUFS.
    pub lufs: Option<f64>,
    /// True peak in dBTP.
    pub true_peak_db: Option<f64>,
    /// The first beat in seconds from the start of the track.
    pub first_beat_sec: Option<f64>,
    /// 12 values, C to B, 0 to 1 (all 0 without a key).
    pub chroma: Vec<f32>,
    /// 12 flags, C to B: the notes of the key's scale.
    pub chroma_in_key: Vec<bool>,
    /// Camelot codes that mix well with this track.
    pub mixes_well_with: Vec<String>,
    /// The drop, once auto cues exist (#48).
    pub drop_sec: Option<f64>,
    /// The mix-out point, once auto cues exist (#48).
    pub mix_out_sec: Option<f64>,
}

/// One track with everything stored about it. The waveform joins with #13.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackDetail {
    /// The browser row.
    pub track: TrackRow,
    /// The file on disk.
    pub path: PathBuf,
    /// The stored analysis, if any.
    pub analysis: Option<Analysis>,
    /// Hot cues, by slot.
    pub cues: Vec<Cue>,
}

/// A crate: a user-made list of tracks (the UI's `Crate`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crate {
    /// The crate's id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// Its colour.
    pub color: CueColor,
    /// How many tracks it holds.
    pub track_count: u32,
}

/// How two neighbours in the set list fit (the UI's `Transition`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    /// The track played first.
    pub from_id: TrackId,
    /// The track played next.
    pub to_id: TrackId,
    /// Same key or a mixing partner on the Camelot wheel; `None` when either
    /// key is unknown.
    pub key_compatible: Option<bool>,
    /// BPM change in percent from the first to the next, to one decimal;
    /// `None` when either tempo is unknown.
    pub bpm_change_pct: Option<f64>,
    /// The change is more than `MAX_BPM_CHANGE_PCT`.
    pub bpm_jump: bool,
}

impl Transition {
    /// How `from` leads into `to` (either may be missing from the library).
    fn between(
        from_id: TrackId,
        from: Option<&TrackRow>,
        to_id: TrackId,
        to: Option<&TrackRow>,
    ) -> Self {
        let camelot = |row: Option<&TrackRow>| {
            row.and_then(|row| row.camelot.as_deref())
                .and_then(|code| code.parse::<Camelot>().ok())
        };
        let key_compatible = match (camelot(from), camelot(to)) {
            (Some(a), Some(b)) => Some(a.matches(b)),
            _ => None,
        };
        let bpm = |row: Option<&TrackRow>| row.and_then(|row| row.bpm).filter(|bpm| *bpm > 0.0);
        let bpm_change_pct = match (bpm(from), bpm(to)) {
            (Some(a), Some(b)) => Some(((b - a) / a * 1000.0).round() / 10.0),
            _ => None,
        };
        Self {
            from_id,
            to_id,
            key_compatible,
            bpm_change_pct,
            bpm_jump: bpm_change_pct.is_some_and(|change| change.abs() > MAX_BPM_CHANGE_PCT),
        }
    }
}

/// One file read and ready to store.
#[derive(Debug)]
struct PreparedFile {
    path: PathBuf,
    info: FileInfo,
    fingerprint: String,
}

/// Files read by [`prepare_add`], ready for [`Library::store`].
#[derive(Debug, Default)]
pub struct PreparedAdd {
    files: Vec<PreparedFile>,
    failed: Vec<(PathBuf, CoreError)>,
}

impl PreparedAdd {
    /// How many files are ready to store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether no file is ready to store.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Finds the new audio files in `paths` and reads them: everything adding
/// does except storing. Skips paths in `known` (from
/// [`Library::known_paths`]) and repeats. Reads each file's tags and headers
/// and its fingerprint on several threads. Needs no library, so the app runs
/// it without holding the library lock.
#[must_use]
pub fn prepare_add(paths: &[PathBuf], known: &HashSet<String>) -> PreparedAdd {
    let mut prepared = PreparedAdd::default();
    let (files, unreadable) = scan::audio_files(paths);
    prepared.failed.extend(
        unreadable
            .into_iter()
            .map(|(path, source)| (path.clone(), CoreError::ReadFile { path, source })),
    );

    // The same file can arrive twice (a file and its folder, or a relative
    // and an absolute spelling): keep the first.
    let mut queued = HashSet::new();
    let mut new_files = Vec::new();
    for file in files {
        let absolute = std::path::absolute(&file).unwrap_or(file);
        let Some(text) = absolute.to_str() else {
            let reason = "the path is not valid Unicode".to_owned();
            prepared.failed.push((
                absolute.clone(),
                CoreError::Decode {
                    path: absolute,
                    reason,
                },
            ));
            continue;
        };
        if !known.contains(text) && queued.insert(text.to_owned()) {
            new_files.push(absolute);
        }
    }

    for (path, outcome) in new_files.iter().zip(read_all(&new_files)) {
        match outcome {
            Ok((info, fingerprint)) => prepared.files.push(PreparedFile {
                path: path.clone(),
                info,
                fingerprint,
            }),
            Err(err) => prepared.failed.push((path.clone(), err)),
        }
    }
    prepared
}

/// What adding files and folders did.
#[derive(Debug, Default)]
pub struct AddResult {
    /// The new tracks, in path order. Files already in the library are not
    /// added again.
    pub added: Vec<TrackId>,
    /// Files and folders that could not be added, with the reason.
    pub failed: Vec<(PathBuf, CoreError)>,
}

/// The track library: one SQLite database.
#[derive(Debug)]
pub struct Library {
    connection: Connection,
}

impl Library {
    /// Opens (or creates) the library at `path` and brings its schema up to
    /// date. Uses WAL mode and foreign keys, and waits up to 5 s for a lock.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] when the file cannot be opened or migrated,
    /// and [`CoreError::DatabaseTooNew`] when a newer Cratekit made it.
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        Self::setup(Connection::open(path)?)
    }

    /// A library in memory, gone when dropped (for tests and previews).
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if SQLite fails to start.
    pub fn open_in_memory() -> Result<Self, CoreError> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(mut connection: Connection) -> Result<Self, CoreError> {
        // In memory, SQLite answers "memory" instead of "wal": both are fine.
        let _mode: String =
            connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        schema::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Adds the audio files in `paths`: files directly, folders with all
    /// their subfolders (see [`AUDIO_EXTENSIONS`]). Files already in the
    /// library are skipped; files that cannot be read are reported in
    /// [`AddResult::failed`] and the rest are still added.
    ///
    /// The same as [`Library::known_paths`], then [`prepare_add`], then
    /// [`Library::store`]. The app calls those three itself, so the slow
    /// middle step (reading every file) runs without holding the library.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading or storing fails (nothing is added
    /// then).
    pub fn add_paths(&mut self, paths: &[PathBuf]) -> Result<AddResult, CoreError> {
        let known = self.known_paths()?;
        let prepared = prepare_add(paths, &known);
        self.store(prepared)
    }

    /// The paths of every track in the library.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading fails.
    pub fn known_paths(&self) -> Result<HashSet<String>, CoreError> {
        let paths = self
            .connection
            .prepare("SELECT path FROM tracks")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(paths)
    }

    /// Stores the files [`prepare_add`] read, in one transaction. A file
    /// another add stored in the meantime is skipped, not an error.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if storing fails (nothing is added then).
    pub fn store(&mut self, prepared: PreparedAdd) -> Result<AddResult, CoreError> {
        let mut result = AddResult {
            added: Vec::with_capacity(prepared.files.len()),
            failed: prepared.failed,
        };
        let transaction = self.connection.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO tracks (path, file_hash, title, artist, album, duration_sec, \
                 sample_rate, channels) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT (path) DO NOTHING RETURNING id",
            )?;
            for file in prepared.files {
                let title = file.info.title.unwrap_or_else(|| file_stem(&file.path));
                let id: Option<TrackId> = insert
                    .query_row(
                        params![
                            file.path.to_str(),
                            file.fingerprint,
                            title,
                            file.info.artist,
                            file.info.album,
                            file.info.duration_sec,
                            file.info.sample_rate,
                            file.info.channels,
                        ],
                        |row| row.get(0),
                    )
                    .optional()?;
                result.added.extend(id);
            }
        }
        transaction.commit()?;
        Ok(result)
    }

    /// The settings: stored values over the defaults (a stored value that no
    /// longer loads keeps its default).
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading fails.
    pub fn settings(&self) -> Result<Settings, CoreError> {
        let pairs: Vec<(String, String)> = self
            .connection
            .prepare("SELECT key, value FROM settings WHERE value IS NOT NULL")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        Ok(Settings::from_stored(pairs))
    }

    /// Changes one setting (`key` as the UI names it) after checking it, and
    /// returns all the settings.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidSetting`] for an unknown key or a wrong value,
    /// [`CoreError::Database`] if writing fails.
    pub fn set_setting(
        &mut self,
        key: &str,
        value: serde_json::Value,
    ) -> Result<Settings, CoreError> {
        let updated = self.settings()?.with(key, value)?;
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, updated.stored_value(key)?],
        )?;
        Ok(updated)
    }

    /// How each pair of neighbours in a set list fits: key and tempo. A
    /// track no longer in the library counts as unknown.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading fails.
    pub fn transitions(&self, ids: &[TrackId]) -> Result<Vec<Transition>, CoreError> {
        let mut rows = Vec::with_capacity(ids.len());
        for &id in ids {
            rows.push(match self.track(id) {
                Ok(detail) => Some(detail.track),
                Err(CoreError::NotFound { .. }) => None,
                Err(err) => return Err(err),
            });
        }
        Ok(ids
            .windows(2)
            .zip(rows.windows(2))
            .map(|(pair, rows)| {
                Transition::between(pair[0], rows[0].as_ref(), pair[1], rows[1].as_ref())
            })
            .collect())
    }

    /// Every crate with how many tracks it holds, in the user's order.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading fails.
    pub fn crates(&self) -> Result<Vec<Crate>, CoreError> {
        let crates = self
            .connection
            .prepare(
                "SELECT c.id, c.name, c.color, \
                   (SELECT count(*) FROM crate_tracks ct WHERE ct.crate_id = c.id) \
                 FROM crates c ORDER BY c.sort IS NULL, c.sort, c.id",
            )?
            .query_map([], |row| {
                Ok(Crate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: CueColor::from_name(row.get::<_, Option<String>>(2)?.as_deref()),
                    track_count: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(crates)
    }

    /// The tracks matching `query`, oldest first.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading fails.
    pub fn list(&self, query: &LibraryQuery) -> Result<Vec<TrackRow>, CoreError> {
        let match_code = query
            .match_camelot
            .as_deref()
            .and_then(|code| code.parse::<Camelot>().ok());
        if query.match_camelot.is_some() && match_code.is_none() {
            return Ok(Vec::new());
        }
        let needle = query.search.trim().to_lowercase();
        let mut statement = self.connection.prepare(&format!(
            "{ROW_SELECT} WHERE (?1 IS NULL OR t.id IN \
               (SELECT track_id FROM crate_tracks WHERE crate_id = ?1)) \
             AND (?2 IS NULL OR a.bpm >= ?2) \
             AND (?3 IS NULL OR a.bpm <= ?3) \
             AND (?4 IS NULL OR abs(a.bpm - ?4) <= ?5) \
             AND (?6 = 0 OR EXISTS (SELECT 1 FROM stems s WHERE s.track_id = t.id)) \
             ORDER BY t.id"
        ))?;
        let rows = statement.query_map(
            params![
                query.crate_id,
                query.bpm_min,
                query.bpm_max,
                query.near_bpm,
                NEAR_BPM_TOLERANCE,
                query.has_stems,
            ],
            |row| Ok((track_row(row)?, row.get::<_, Option<String>>(12)?)),
        )?;
        let mut tracks = Vec::new();
        for row in rows {
            let (track, album) = row?;
            if !needle.is_empty() {
                let haystack = format!(
                    "{} {} {}",
                    track.title,
                    track.artist,
                    album.unwrap_or_default()
                )
                .to_lowercase();
                if !needle
                    .split_whitespace()
                    .all(|word| haystack.contains(word))
                {
                    continue;
                }
            }
            if let Some(wanted) = match_code {
                let mixes = track
                    .camelot
                    .as_deref()
                    .and_then(|code| code.parse::<Camelot>().ok())
                    .is_some_and(|code| code.matches(wanted));
                if !mixes {
                    continue;
                }
            }
            tracks.push(track);
        }
        Ok(tracks)
    }

    /// The sidebar counts.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if reading fails.
    pub fn counts(&self) -> Result<LibraryCounts, CoreError> {
        let counts = self.connection.query_row(
            "SELECT count(*), \
               count(*) FILTER (WHERE t.added_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)), \
               count(*) FILTER (WHERE a.track_id IS NULL OR a.analyzer_version < ?2) \
             FROM tracks t LEFT JOIN analysis a ON a.track_id = t.id",
            params![format!("-{RECENT_DAYS} days"), ANALYZER_VERSION],
            |row| {
                Ok(LibraryCounts {
                    all: row.get(0)?,
                    recently_added: row.get(1)?,
                    needs_analysis: row.get(2)?,
                })
            },
        )?;
        Ok(counts)
    }

    /// One track with its path and cues.
    ///
    /// # Errors
    ///
    /// [`CoreError::NotFound`] for an unknown id, [`CoreError::Database`] if
    /// reading fails.
    pub fn track(&self, id: TrackId) -> Result<TrackDetail, CoreError> {
        let found = self
            .connection
            .query_row(&format!("{ROW_SELECT} WHERE t.id = ?1"), [id], |row| {
                Ok((track_row(row)?, row.get::<_, String>(13)?))
            })
            .optional()?;
        let Some((track, path)) = found else {
            return Err(CoreError::NotFound { what: "track", id });
        };
        let cues = self
            .connection
            .prepare(
                "SELECT slot, position_sec, color, name FROM cues WHERE track_id = ?1 \
                 AND slot BETWEEN 1 AND 8 ORDER BY slot",
            )?
            .query_map([id], |row| {
                Ok(Cue {
                    slot: row.get(0)?,
                    position_sec: row.get(1)?,
                    color: CueColor::from_name(row.get::<_, Option<String>>(2)?.as_deref()),
                    name: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(TrackDetail {
            track,
            path: PathBuf::from(path),
            analysis: self.analysis(id)?,
            cues,
        })
    }

    /// The stored analysis of a track, if any.
    fn analysis(&self, id: TrackId) -> Result<Option<Analysis>, CoreError> {
        let row = self
            .connection
            .query_row(
                "SELECT bpm, bpm_confidence, key_tonic, key_mode, key_confidence, energy, lufs, \
                 true_peak_db, first_beat_sec, chroma FROM analysis WHERE track_id = ?1",
                [id],
                |row| {
                    let key = stored_key(row.get(2)?, row.get::<_, Option<String>>(3)?.as_deref());
                    let chroma: Option<Vec<u8>> = row.get(9)?;
                    Ok(Analysis {
                        bpm: row.get(0)?,
                        bpm_confidence: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                        key_name: key.map(|key| key.to_string()),
                        camelot: key.map(|key| key.camelot().to_string()),
                        key_confidence: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                        energy: row.get(5)?,
                        lufs: row.get(6)?,
                        true_peak_db: row.get(7)?,
                        first_beat_sec: row.get(8)?,
                        chroma: chroma
                            .map_or_else(|| vec![0.0; 12], |bytes| chroma_from_bytes(&bytes)),
                        chroma_in_key: key
                            .map_or_else(|| vec![false; 12], |key| key.scale().to_vec()),
                        mixes_well_with: key.map_or_else(Vec::new, |key| {
                            key.camelot()
                                .mixes_well_with()
                                .iter()
                                .map(ToString::to_string)
                                .collect()
                        }),
                        drop_sec: None,
                        mix_out_sec: None,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Stores the analysis of a track (replacing an older one) and updates
    /// its length, sample rate and channels from the decoded audio.
    ///
    /// # Errors
    ///
    /// [`CoreError::NotFound`] when the track is gone (removed while it was
    /// analyzed), [`CoreError::Database`] if writing fails.
    pub fn store_analysis(&mut self, id: TrackId, found: &TrackAnalysis) -> Result<(), CoreError> {
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE tracks SET duration_sec = ?2, sample_rate = ?3, channels = ?4 WHERE id = ?1",
            params![id, found.duration_sec, found.sample_rate, found.channels],
        )?;
        if updated == 0 {
            return Err(CoreError::NotFound { what: "track", id });
        }
        let key = found.key.as_ref();
        transaction.execute(
            "INSERT OR REPLACE INTO analysis (track_id, bpm, bpm_confidence, key_tonic, key_mode, \
             camelot, key_confidence, energy, lufs, true_peak_db, first_beat_sec, chroma, \
             analyzer_version, analyzed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, \
             ?11, ?12, ?13, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            params![
                id,
                found.tempo.map(|tempo| f64::from(tempo.bpm)),
                found.tempo.map(|tempo| f64::from(tempo.confidence)),
                key.map(|key| key.key.tonic.index()),
                key.map(|key| key.key.mode.to_string()),
                key.map(|key| key.key.camelot().to_string()),
                key.map(|key| f64::from(key.confidence)),
                found.energy,
                found
                    .loudness
                    .map(|loudness| f64::from(loudness.integrated_lufs)),
                found
                    .loudness
                    .map(|loudness| f64::from(loudness.true_peak_dbtp)),
                found
                    .tempo
                    .and_then(|tempo| tempo.first_beat_sec)
                    .map(f64::from),
                key.map(|key| chroma_to_bytes(&key.chroma)),
                ANALYZER_VERSION,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Removes tracks from the library: their rows, analysis, cues and crate
    /// entries. The audio files on disk are never touched. Returns how many
    /// tracks were removed.
    ///
    /// # Errors
    ///
    /// [`CoreError::Database`] if writing fails (nothing is removed then).
    pub fn remove_tracks(&mut self, ids: &[TrackId]) -> Result<usize, CoreError> {
        let transaction = self.connection.transaction()?;
        let mut removed = 0;
        {
            let mut delete = transaction.prepare("DELETE FROM tracks WHERE id = ?1")?;
            for id in ids {
                removed += delete.execute([id])?;
            }
        }
        transaction.commit()?;
        Ok(removed)
    }

    /// The connection, for tests that set up analysis, stems or crates.
    #[cfg(test)]
    fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// The columns [`track_row`] reads, then the album (12) and path (13).
const ROW_SELECT: &str = "SELECT t.id, t.title, t.artist, t.duration_sec, a.bpm, a.key_tonic, \
     a.key_mode, a.energy, a.lufs, a.track_id IS NOT NULL, \
     EXISTS (SELECT 1 FROM stems s WHERE s.track_id = t.id), \
     EXISTS (SELECT 1 FROM masters m WHERE m.track_id = t.id), t.album, t.path \
     FROM tracks t LEFT JOIN analysis a ON a.track_id = t.id";

/// A key stored as a pitch class index (0 = C) and "major" or "minor".
fn stored_key(tonic: Option<u8>, mode: Option<&str>) -> Option<Key> {
    let mode = match mode? {
        "major" => Mode::Major,
        "minor" => Mode::Minor,
        _ => return None,
    };
    Some(Key {
        tonic: PitchClass::from_index(tonic?),
        mode,
    })
}

/// Chroma as stored: 12 little-endian `f32`.
fn chroma_to_bytes(chroma: &[f32; 12]) -> Vec<u8> {
    chroma
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// Chroma from its stored bytes; zeros for anything malformed.
fn chroma_from_bytes(bytes: &[u8]) -> Vec<f32> {
    if bytes.len() != 48 {
        return vec![0.0; 12];
    }
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

fn track_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
    let key = stored_key(row.get(5)?, row.get::<_, Option<String>>(6)?.as_deref());
    Ok(TrackRow {
        id: row.get(0)?,
        title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        artist: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        duration_sec: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
        bpm: row.get(4)?,
        camelot: key.map(|key| key.camelot().to_string()),
        key_name: key.map(|key| key.to_string()),
        energy: row.get(7)?,
        lufs: row.get(8)?,
        analyzed: row.get(9)?,
        has_stems: row.get(10)?,
        mastered: row.get(11)?,
    })
}

/// The file name without its extension: the title of an untagged file.
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Probes and fingerprints `files` on up to [`MAX_PROBE_THREADS`] threads,
/// keeping their order.
fn read_all(files: &[PathBuf]) -> Vec<Result<(FileInfo, String), CoreError>> {
    let threads = thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, MAX_PROBE_THREADS);
    let chunk = files.len().div_ceil(threads).max(1);
    thread::scope(|scope| {
        let workers: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                let worker = scope.spawn(move || part.iter().map(|path| read_one(path)).collect());
                (part.len(), worker)
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|(count, worker)| {
                // A panicking reader fails its own files only, one result each.
                worker.join().unwrap_or_else(|_| {
                    (0..count)
                        .map(|_| Err(CoreError::InvalidAudio("a file reader thread panicked")))
                        .collect::<Vec<_>>()
                })
            })
            .collect()
    })
}

fn read_one(path: &Path) -> Result<(FileInfo, String), CoreError> {
    let info = probe_file(path)?;
    let hash = scan::fingerprint(path).map_err(|source| CoreError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    Ok((info, hash))
}

#[cfg(test)]
mod tests;
