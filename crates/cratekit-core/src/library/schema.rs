//! The SQLite schema and its migrations.
//!
//! Each migration is one SQL step. The database's `PRAGMA user_version`
//! records how many have run; opening a database runs the missing ones, each
//! in its own transaction. Never edit a released step: add a new one.

use rusqlite::Connection;

use crate::CoreError;

/// The migrations, in order. `user_version` N means the first N have run.
const MIGRATIONS: &[&str] = &[
    // 1: the schema from docs/ARCHITECTURE.md (Database).
    r"
CREATE TABLE tracks (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  file_hash TEXT NOT NULL,
  title TEXT, artist TEXT, album TEXT,
  duration_sec REAL, sample_rate INTEGER, channels INTEGER,
  added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE analysis (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  bpm REAL, bpm_confidence REAL,
  key_tonic INTEGER, key_mode TEXT, camelot TEXT, key_confidence REAL,
  energy INTEGER, lufs REAL, true_peak_db REAL,
  first_beat_sec REAL,
  waveform BLOB,
  analyzer_version INTEGER NOT NULL,
  analyzed_at TEXT NOT NULL
);
CREATE TABLE cues (
  track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
  slot INTEGER,
  position_sec REAL NOT NULL, color TEXT, name TEXT,
  PRIMARY KEY (track_id, slot)
);
CREATE TABLE stems (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  model TEXT NOT NULL, dir TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE masters (
  id INTEGER PRIMARY KEY, track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
  preset TEXT, settings_json TEXT, path TEXT, lufs REAL, created_at TEXT
);
CREATE TABLE crates (id INTEGER PRIMARY KEY, name TEXT NOT NULL, color TEXT, sort INTEGER);
CREATE TABLE crate_tracks (crate_id INTEGER REFERENCES crates(id) ON DELETE CASCADE,
  track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE, sort INTEGER,
  PRIMARY KEY (crate_id, track_id));
CREATE TABLE midi_mappings (device TEXT, target TEXT, kind TEXT, channel INTEGER, number INTEGER,
  PRIMARY KEY (device, target));
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
CREATE INDEX masters_track ON masters(track_id);
CREATE INDEX crate_tracks_track ON crate_tracks(track_id);
",
];

/// The schema version this build knows: the number of migrations.
pub(crate) fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// Runs the migrations `conn` has not had yet.
///
/// # Errors
///
/// [`CoreError::DatabaseTooNew`] when the file was made by a newer Cratekit
/// (its tables may not mean what this build thinks), and
/// [`CoreError::Database`] when SQLite fails; a failed step is rolled back.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), CoreError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let known = latest_version();
    if current > known {
        return Err(CoreError::DatabaseTooNew {
            found: current,
            known,
        });
    }
    for (done, step) in (current..).zip(MIGRATIONS.iter().skip(current.max(0) as usize)) {
        let transaction = conn.transaction()?;
        transaction.execute_batch(step)?;
        transaction.pragma_update(None, "user_version", done + 1)?;
        transaction.commit()?;
    }
    Ok(())
}
