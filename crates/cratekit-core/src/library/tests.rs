//! Tests for the track library, on the synthetic fixture files.

use std::time::Instant;

use super::*;
use crate::testing::{Scratch, fixture, read_fixture};

fn library() -> Library {
    Library::open_in_memory().expect("an in-memory library")
}

/// Stores an analysis row the way the analysis task (#15) will.
fn analyze(library: &Library, id: TrackId, bpm: f64, tonic: u8, mode: &str) {
    library
        .connection()
        .execute(
            "INSERT INTO analysis (track_id, bpm, key_tonic, key_mode, energy, lufs, \
             analyzer_version, analyzed_at) VALUES (?1, ?2, ?3, ?4, 7, -9.5, ?5, 'now')",
            params![id, bpm, tonic, mode, ANALYZER_VERSION],
        )
        .expect("store an analysis");
}

/// A folder with tagged and untagged audio, a hidden file, a text file and
/// a "song" that is not audio.
fn music_folder(scratch: &Scratch) -> PathBuf {
    let root = scratch.path().join("Music");
    scratch.write("Music/Tagged.mp3", &read_fixture("tagged.mp3"));
    scratch.write("Music/House/untagged tone.wav", &read_fixture("tone.wav"));
    scratch.write("Music/House/Deep/tagged.FLAC", &read_fixture("tagged.flac"));
    scratch.write("Music/.hidden.mp3", &read_fixture("tagged.mp3"));
    scratch.write("Music/notes.txt", b"not audio");
    scratch.write("Music/broken.mp3", b"not really an mp3");
    root
}

#[test]
fn a_new_library_has_the_latest_schema_and_nothing_in_it() {
    let library = library();
    let version: i64 = library
        .connection()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(version, schema::latest_version());
    let foreign_keys: bool = library
        .connection()
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("read the pragma");
    assert!(foreign_keys);
    assert_eq!(
        library.counts().expect("counts"),
        LibraryCounts {
            all: 0,
            recently_added: 0,
            needs_analysis: 0
        }
    );
}

#[test]
fn a_file_library_uses_wal_and_reopens_without_migrating_again() {
    let scratch = Scratch::new("library-file");
    let path = scratch.path().join("cratekit.db");
    let mut library = Library::open(&path).expect("create the library");
    let added = library
        .add_paths(&[fixture("tagged.mp3")])
        .expect("add a track")
        .added;
    let mode: String = library
        .connection()
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("read the mode");
    assert_eq!(mode, "wal");
    drop(library);
    let reopened = Library::open(&path).expect("reopen the library");
    assert_eq!(reopened.counts().expect("counts").all, 1);
    assert_eq!(
        reopened.list(&LibraryQuery::default()).expect("list")[0].id,
        added[0]
    );
}

#[test]
fn a_library_from_a_newer_cratekit_is_refused() {
    let scratch = Scratch::new("library-too-new");
    let path = scratch.path().join("cratekit.db");
    let connection = Connection::open(&path).expect("create a file");
    connection
        .pragma_update(None, "user_version", 99)
        .expect("pretend a newer schema");
    drop(connection);
    let result = Library::open(&path);
    assert!(
        matches!(result, Err(CoreError::DatabaseTooNew { found: 99, .. })),
        "{result:?}"
    );
}

#[test]
fn adds_the_audio_files_of_a_folder_with_their_tags() {
    let scratch = Scratch::new("library-folder");
    let mut library = library();
    let result = library
        .add_paths(&[music_folder(&scratch)])
        .expect("add the folder");
    assert_eq!(result.added.len(), 3, "{:?}", result.failed);
    assert_eq!(result.failed.len(), 1, "only the fake MP3 fails");
    assert!(result.failed[0].0.ends_with("broken.mp3"));
    assert!(
        matches!(result.failed[0].1, CoreError::UnsupportedFormat { .. }),
        "{:?}",
        result.failed[0].1
    );

    let rows = library.list(&LibraryQuery::default()).expect("list");
    let titles: Vec<_> = rows.iter().map(|row| row.title.as_str()).collect();
    // Path order: Deep/tagged.FLAC, untagged tone.wav, Tagged.mp3.
    assert_eq!(titles, ["Test Tone", "untagged tone", "Test Tone"]);
    assert_eq!(rows[0].artist, "Cratekit");
    assert_eq!(rows[1].artist, "", "an untagged file has no artist");
    assert!((rows[1].duration_sec - 0.5).abs() < 1e-6);
    assert!(rows.iter().all(|row| !row.analyzed && row.bpm.is_none()));
    let counts = library.counts().expect("counts");
    assert_eq!(
        (counts.all, counts.recently_added, counts.needs_analysis),
        (3, 3, 3)
    );
}

#[test]
fn adding_the_same_files_again_adds_nothing() {
    let scratch = Scratch::new("library-again");
    let mut library = library();
    let folder = music_folder(&scratch);
    library
        .add_paths(std::slice::from_ref(&folder))
        .expect("first add");
    let again = library
        .add_paths(&[folder.clone(), folder.join("Tagged.mp3")])
        .expect("second add");
    assert!(again.added.is_empty());
    assert_eq!(library.counts().expect("counts").all, 3);
}

#[test]
fn one_file_spelled_two_ways_is_added_once() {
    // Tests run in the crate folder, so this relative path is the same file
    // as the absolute fixture path; another file sorts between them.
    let mut library = library();
    let result = library
        .add_paths(&[
            fixture("tagged.mp3"),
            fixture("tagged.flac"),
            PathBuf::from("tests/fixtures/tagged.mp3"),
        ])
        .expect("add");
    assert_eq!(result.added.len(), 2, "{:?}", result.failed);
    assert!(result.failed.is_empty());
}

#[test]
fn a_file_stored_by_another_add_in_the_meantime_is_skipped() {
    // Two adds read the same file before either stores it (the app reads
    // without holding the library).
    let mut library = library();
    let known = library.known_paths().expect("known paths");
    let first = prepare_add(&[fixture("tagged.mp3")], &known);
    let second = prepare_add(&[fixture("tagged.mp3")], &known);
    assert_eq!((first.len(), second.len()), (1, 1));
    assert_eq!(library.store(first).expect("store").added.len(), 1);
    let again = library.store(second).expect("store");
    assert!(again.added.is_empty() && again.failed.is_empty());
    assert_eq!(library.counts().expect("counts").all, 1);
}

#[test]
fn crates_list_their_track_counts_in_order() {
    let mut library = library();
    assert!(library.crates().expect("crates").is_empty());
    let ids = library
        .add_paths(&[fixture("tagged.mp3"), fixture("tagged.flac")])
        .expect("add")
        .added;
    library
        .connection()
        .execute_batch(&format!(
            "INSERT INTO crates (id, name, color, sort) VALUES (1, 'Warm up', 'teal', 2), \
               (2, 'Peak', NULL, 1), (3, 'Unsorted', 'red', NULL); \
             INSERT INTO crate_tracks (crate_id, track_id) VALUES (1, {}), (1, {}), (2, {});",
            ids[0], ids[1], ids[0]
        ))
        .expect("set up crates");
    let crates = library.crates().expect("crates");
    let summary: Vec<_> = crates
        .iter()
        .map(|c| (c.name.as_str(), c.color, c.track_count))
        .collect();
    assert_eq!(
        summary,
        [
            ("Peak", CueColor::Orange, 1),
            ("Warm up", CueColor::Teal, 2),
            ("Unsorted", CueColor::Red, 0),
        ]
    );
}

#[test]
fn search_matches_title_artist_and_album_in_any_case() {
    let mut library = library();
    library
        .add_paths(&[fixture("tagged.mp3"), fixture("tone.wav")])
        .expect("add");
    let search = |text: &str| {
        library
            .list(&LibraryQuery {
                search: text.into(),
                ..LibraryQuery::default()
            })
            .expect("list")
            .len()
    };
    assert_eq!(search("test tone"), 1);
    assert_eq!(search("CRATEKIT"), 1);
    assert_eq!(search("fixtures"), 1, "album");
    assert_eq!(search("tone"), 2, "the untagged file's name");
    assert_eq!(search("tone cratekit"), 1, "every word must match");
    assert_eq!(search("nothing like this"), 0);
    assert_eq!(search("   "), 2, "blank search shows everything");
}

#[test]
fn filters_by_bpm_key_stems_and_crate() {
    let mut library = library();
    let ids = library
        .add_paths(&[
            fixture("tagged.mp3"),
            fixture("tagged.flac"),
            fixture("tagged.m4a"),
            fixture("tone.wav"),
        ])
        .expect("add")
        .added;
    // 124 BPM A minor (8A), 128 BPM C major (8B), 140 BPM F# minor (11A);
    // the fourth track is not analyzed.
    analyze(&library, ids[0], 124.0, 9, "minor");
    analyze(&library, ids[1], 128.0, 0, "major");
    analyze(&library, ids[2], 140.0, 6, "minor");
    let list = |query: LibraryQuery| -> Vec<TrackId> {
        library
            .list(&query)
            .expect("list")
            .iter()
            .map(|row| row.id)
            .collect()
    };

    let rows = library.list(&LibraryQuery::default()).expect("list");
    assert_eq!(rows[0].camelot.as_deref(), Some("8A"));
    assert_eq!(rows[0].key_name.as_deref(), Some("A minor"));
    assert_eq!((rows[0].energy, rows[0].lufs), (Some(7), Some(-9.5)));
    assert!(rows[0].analyzed && !rows[3].analyzed);

    let bpm = |min, max| LibraryQuery {
        bpm_min: min,
        bpm_max: max,
        ..LibraryQuery::default()
    };
    assert_eq!(list(bpm(Some(125.0), None)), [ids[1], ids[2]]);
    assert_eq!(list(bpm(None, Some(130.0))), [ids[0], ids[1]]);
    let near = |deck| LibraryQuery {
        near_bpm: Some(deck),
        ..LibraryQuery::default()
    };
    assert_eq!(list(near(126.0)), [ids[0], ids[1]], "within 3 BPM");
    assert_eq!(list(near(132.0)), Vec::<TrackId>::new());
    let key = |code: &str| LibraryQuery {
        match_camelot: Some(code.into()),
        ..LibraryQuery::default()
    };
    assert_eq!(list(key("8A")), [ids[0], ids[1]], "itself and its relative");
    assert_eq!(list(key("12A")), [ids[2]], "one step away");
    assert_eq!(list(key("not a code")), Vec::<TrackId>::new());

    library
        .connection()
        .execute_batch(&format!(
            "INSERT INTO stems VALUES ({}, 'htdemucs', '/stems', 'now'); \
             INSERT INTO crates (id, name) VALUES (1, 'Friday'); \
             INSERT INTO crate_tracks (crate_id, track_id) VALUES (1, {}), (1, {});",
            ids[2], ids[1], ids[3]
        ))
        .expect("set up stems and a crate");
    let stems = LibraryQuery {
        has_stems: true,
        ..LibraryQuery::default()
    };
    assert_eq!(list(stems), [ids[2]]);
    let in_crate = LibraryQuery {
        crate_id: Some(1),
        ..LibraryQuery::default()
    };
    assert_eq!(list(in_crate), [ids[1], ids[3]]);
    let counts = library.counts().expect("counts");
    assert_eq!(counts.needs_analysis, 1);
}

#[test]
fn track_detail_has_the_path_and_cues() {
    let mut library = library();
    let id = library
        .add_paths(&[fixture("tagged.mp3")])
        .expect("add")
        .added[0];
    library
        .connection()
        .execute_batch(&format!(
            "INSERT INTO cues VALUES ({id}, 3, 12.5, 'blue', 'Drop'), ({id}, 1, 0.25, NULL, NULL);"
        ))
        .expect("store cues");
    let detail = library.track(id).expect("the track");
    assert_eq!(detail.track.title, "Test Tone");
    assert!(detail.path.ends_with("tests/fixtures/tagged.mp3"));
    assert_eq!(
        detail.cues,
        [
            Cue {
                slot: 1,
                position_sec: 0.25,
                color: CueColor::Orange,
                name: None
            },
            Cue {
                slot: 3,
                position_sec: 12.5,
                color: CueColor::Blue,
                name: Some("Drop".into())
            },
        ]
    );
    let missing = library.track(id + 100);
    assert!(
        matches!(missing, Err(CoreError::NotFound { what: "track", .. })),
        "{missing:?}"
    );
}

#[test]
fn removing_tracks_never_deletes_the_files() {
    let scratch = Scratch::new("library-remove");
    let file = scratch.write("keep me.mp3", &read_fixture("tagged.mp3"));
    let mut library = library();
    let id = library
        .add_paths(std::slice::from_ref(&file))
        .expect("add")
        .added[0];
    analyze(&library, id, 124.0, 9, "minor");
    assert_eq!(library.remove_tracks(&[id, id + 1]).expect("remove"), 1);
    assert!(file.is_file(), "the file is still on disk");
    assert_eq!(library.counts().expect("counts").all, 0);
    let analyses: i64 = library
        .connection()
        .query_row("SELECT count(*) FROM analysis", [], |row| row.get(0))
        .expect("count");
    assert_eq!(analyses, 0, "its analysis went with it");
}

#[test]
fn five_hundred_tracks_are_listed_within_five_seconds() {
    // SPEC Module 1: adding a folder of 500 tracks shows them within 5 s.
    let scratch = Scratch::new("library-500");
    let bytes = read_fixture("tagged.mp3");
    for n in 0..500 {
        scratch.write(&format!("Crate/{:02}/track {n:03}.mp3", n % 20), &bytes);
    }
    let mut library = library();
    let start = Instant::now();
    let result = library
        .add_paths(&[scratch.path().join("Crate")])
        .expect("add 500 tracks");
    let rows = library.list(&LibraryQuery::default()).expect("list");
    let elapsed = start.elapsed();
    assert_eq!((result.added.len(), rows.len()), (500, 500));
    assert!(elapsed.as_secs_f64() < 5.0, "took {elapsed:?}");
}
