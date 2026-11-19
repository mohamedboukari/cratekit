//! The background jobs: analysis jobs run one at a time on a worker thread
//! and can be paused; the model download runs on its own thread. All report
//! progress as events and can be cancelled.
//!
//! Events (see docs/ARCHITECTURE.md, IPC): `job_progress`, `job_done`,
//! `job_failed`. A cancelled job sends nothing more (the UI already dropped
//! it).

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use cratekit_core::CoreError;
use cratekit_core::analysis::track::{TrackAnalysis, analyze_file};
use cratekit_core::library::{Library, TrackId};
use cratekit_core::progress::{CancellationToken, ProgressSink};
use cratekit_core::stems::models::{Model, Stage};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppError;
use crate::library::LibraryState;

/// Steps of progress per track in `job_progress` (`done` and `total` count
/// these, so a single track still moves smoothly).
const STEPS_PER_TRACK: u32 = 100;

/// What a job does (the UI's `JobKind`). Stems, mastering and export add
/// their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum JobKind {
    /// Tempo, key, loudness and energy of tracks.
    Analysis,
    /// The stems model download.
    ModelDownload,
}

/// A step inside a multi-step job (the UI's `JobStage`): the model download
/// receives the files, then checks their SHA-256.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum JobStage {
    /// Receiving the files.
    Download,
    /// Checking their checksums.
    Verify,
}

impl From<Stage> for JobStage {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::Download => Self::Download,
            Stage::Verify => Self::Verify,
        }
    }
}

/// `job_progress`: the UI's `JobProgress`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobProgress {
    job_id: String,
    kind: JobKind,
    /// English fallback; the UI names jobs from `kind` and the track count.
    label: String,
    stage: Option<JobStage>,
    track_ids: Vec<TrackId>,
    /// Progress steps (not tracks): analysis counts `STEPS_PER_TRACK` per
    /// track, the model download `DOWNLOAD_STEPS` in all.
    done: u32,
    total: u32,
    /// Waiting for an earlier job (or a resume), not started yet.
    queued: bool,
}

impl JobProgress {
    /// An analysis job's progress event.
    fn analysis(job_id: &str, track_ids: &[TrackId], done: u32, queued: bool) -> Self {
        let count = u32::try_from(track_ids.len()).unwrap_or(u32::MAX);
        Self {
            job_id: job_id.to_owned(),
            kind: JobKind::Analysis,
            label: format!("Analyzing {count} tracks"),
            stage: None,
            track_ids: track_ids.to_vec(),
            done,
            total: count.saturating_mul(STEPS_PER_TRACK),
            queued,
        }
    }
}

impl JobProgress {
    /// A model download's progress event (`fraction` of the whole model).
    fn model_download(job_id: &str, model: &str, stage: Stage, fraction: f32) -> Self {
        let label = match stage {
            Stage::Download => format!("Downloading {model}"),
            Stage::Verify => format!("Verifying {model} checksum"),
        };
        Self {
            job_id: job_id.to_owned(),
            kind: JobKind::ModelDownload,
            label,
            stage: Some(stage.into()),
            track_ids: Vec::new(),
            done: step_of(fraction),
            total: DOWNLOAD_STEPS,
            queued: false,
        }
    }
}

/// Progress steps of a whole model download.
const DOWNLOAD_STEPS: u32 = 1000;

/// The step (0 to `DOWNLOAD_STEPS`) a fraction of the download reached.
fn step_of(fraction: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0 to DOWNLOAD_STEPS, which u32 holds"
    )]
    let step = (fraction.clamp(0.0, 1.0) * DOWNLOAD_STEPS as f32).round() as u32;
    step
}

/// `job_done`: the UI's `JobDone`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobDone {
    job_id: String,
    kind: JobKind,
}

/// `job_failed`: the UI's `JobFailed`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobFailed {
    job_id: String,
    kind: JobKind,
    error: AppError,
}

/// Where a job's events go: the app's windows, or a recorder in tests.
pub(crate) trait JobEvents: Send + Sync {
    fn progress(&self, event: JobProgress);
    fn done(&self, event: JobDone);
    fn failed(&self, event: JobFailed);
}

impl<R: Runtime> JobEvents for AppHandle<R> {
    // An event that cannot be delivered (the window is closing) has no one
    // left to tell, so a failed emit is ignored.
    fn progress(&self, event: JobProgress) {
        self.emit("job_progress", event).ok();
    }
    fn done(&self, event: JobDone) {
        self.emit("job_done", event).ok();
    }
    fn failed(&self, event: JobFailed) {
        self.emit("job_failed", event).ok();
    }
}

/// What analyzes one file: `analyze_file` in the app, a stand-in in tests.
type Analyzer =
    fn(&Path, &dyn ProgressSink, &CancellationToken) -> Result<TrackAnalysis, CoreError>;

/// One queued or running job.
#[derive(Debug, Clone)]
struct Job {
    id: String,
    track_ids: Vec<TrackId>,
    cancel: CancellationToken,
}

/// The queue, shared by the commands and the worker.
#[derive(Debug, Default)]
struct Queue {
    waiting: VecDeque<Job>,
    running: Option<Job>,
    paused: bool,
    /// The model download, which runs on its own thread (not behind the
    /// analysis queue): its job id and how to cancel it.
    download: Option<(String, CancellationToken)>,
}

/// The job queue in the app's state.
#[derive(Debug, Default, Clone)]
pub(crate) struct Jobs {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    next_id: Arc<AtomicU64>,
}

impl Jobs {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        // The queue holds plain data; a panic elsewhere leaves it usable.
        self.queue.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues an analysis of `track_ids` and returns its job id. Tells the
    /// UI right away (a queued `job_progress`), before the worker can start
    /// it, so a waiting job can be seen and cancelled.
    pub(crate) fn queue_analysis(&self, track_ids: Vec<TrackId>, events: &dyn JobEvents) -> String {
        let id = format!("job-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        events.progress(JobProgress::analysis(&id, &track_ids, 0, true));
        self.lock().waiting.push_back(Job {
            id: id.clone(),
            track_ids,
            cancel: CancellationToken::new(),
        });
        self.queue.1.notify_all();
        id
    }

    /// Cancels a job: a waiting one never starts, a running one stops at its
    /// next check. Unknown or finished ids are ignored.
    pub(crate) fn cancel(&self, job_id: &str) {
        let mut queue = self.lock();
        queue.waiting.retain(|job| job.id != job_id);
        if let Some(job) = queue.running.as_ref().filter(|job| job.id == job_id) {
            job.cancel.cancel();
        }
        if let Some((_, cancel)) = queue.download.as_ref().filter(|(id, _)| id == job_id) {
            cancel.cancel();
        }
        // A running job waiting for a resume wakes up and stops.
        self.queue.1.notify_all();
    }

    /// Pauses or resumes the queue. A paused queue starts no new track; the
    /// track being analyzed finishes first.
    pub(crate) fn set_paused(&self, paused: bool) {
        self.lock().paused = paused;
        self.queue.1.notify_all();
    }

    /// Downloads `model` into `models_dir` on its own thread and returns the
    /// job id. While a download runs, asking again returns its id instead of
    /// starting a second one (both would write the same files).
    pub(crate) fn start_download<E: JobEvents + Clone + 'static>(
        &self,
        model: Model,
        models_dir: PathBuf,
        events: E,
    ) -> String {
        let (id, cancel) = {
            let mut queue = self.lock();
            if let Some((id, _)) = &queue.download {
                return id.clone();
            }
            let id = format!("job-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
            let cancel = CancellationToken::new();
            queue.download = Some((id.clone(), cancel.clone()));
            (id, cancel)
        };
        events.progress(JobProgress::model_download(
            &id,
            model.name,
            Stage::Download,
            0.0,
        ));
        let jobs = self.clone();
        let job_id = id.clone();
        std::thread::spawn(move || {
            jobs.run_download(&job_id, model, &models_dir, &events, &cancel);
            jobs.lock().download = None;
        });
        id
    }

    /// Whether a model download is running.
    pub(crate) fn is_downloading(&self) -> bool {
        self.lock().download.is_some()
    }

    /// Runs one model download and reports it (cancelled: nothing more).
    fn run_download(
        &self,
        job_id: &str,
        model: Model,
        models_dir: &Path,
        events: &dyn JobEvents,
        cancel: &CancellationToken,
    ) {
        // Report whole steps only: about a thousand events for the download.
        let last = AtomicU64::new(u64::MAX);
        let progress = |stage: Stage, fraction: f32| {
            let key = u64::from(step_of(fraction)) | (u64::from(stage == Stage::Verify) << 32);
            if last.swap(key, Ordering::Relaxed) != key {
                events.progress(JobProgress::model_download(
                    job_id, model.name, stage, fraction,
                ));
            }
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            model.download(models_dir, &progress, cancel)
        }))
        .unwrap_or(Err(CoreError::Download {
            url: String::new(),
            reason: "the download stopped unexpectedly".to_owned(),
        }));
        match outcome {
            Ok(()) => events.done(JobDone {
                job_id: job_id.to_owned(),
                kind: JobKind::ModelDownload,
            }),
            Err(CoreError::Cancelled) => {}
            Err(err) => events.failed(JobFailed {
                job_id: job_id.to_owned(),
                kind: JobKind::ModelDownload,
                error: err.into(),
            }),
        }
    }

    /// Waits until the queue is not paused (or `cancel` fires). Returns
    /// false when the job was cancelled meanwhile.
    fn wait_while_paused(&self, cancel: &CancellationToken) -> bool {
        let mut queue = self.lock();
        while queue.paused && !cancel.is_cancelled() {
            queue = self
                .queue
                .1
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
        !cancel.is_cancelled()
    }

    /// Takes the next job, waiting for one (and for a resume) as needed.
    fn next(&self) -> Job {
        let mut queue = self.lock();
        loop {
            if !queue.paused
                && let Some(job) = queue.waiting.pop_front()
            {
                queue.running = Some(job.clone());
                return job;
            }
            queue = self
                .queue
                .1
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn finish(&self) {
        self.lock().running = None;
    }

    /// Starts the worker thread that runs the jobs, for as long as the app
    /// runs.
    pub(crate) fn start<R: Runtime>(&self, app: AppHandle<R>) {
        let jobs = self.clone();
        std::thread::spawn(move || {
            loop {
                let job = jobs.next();
                // A panic must not end the worker: the queue would stop for
                // the rest of the session. Tracks are already guarded one by
                // one; this catches anything else in the job.
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let state = app.state::<LibraryState>();
                    match state.library() {
                        Ok(library) => jobs.run_analysis(&job, library, &app, analyze_file),
                        Err(error) => app.failed(JobFailed {
                            job_id: job.id.clone(),
                            kind: JobKind::Analysis,
                            error,
                        }),
                    }
                }));
                if outcome.is_err() {
                    app.failed(JobFailed {
                        job_id: job.id.clone(),
                        kind: JobKind::Analysis,
                        error: AppError {
                            code: "internal",
                            message: "the analysis job stopped unexpectedly".into(),
                        },
                    });
                }
                jobs.finish();
            }
        });
    }

    /// Analyzes every track of `job` and stores the results. Tracks that
    /// fail are skipped; the job then ends with `job_failed` naming how many
    /// failed and the first reason, else with `job_done`.
    fn run_analysis(
        &self,
        job: &Job,
        library: &Mutex<Library>,
        events: &dyn JobEvents,
        analyze: Analyzer,
    ) {
        let count = u32::try_from(job.track_ids.len()).unwrap_or(u32::MAX);
        let report = |done: u32| {
            events.progress(JobProgress::analysis(&job.id, &job.track_ids, done, false));
        };
        report(0);
        let mut failures: Vec<CoreError> = Vec::new();
        for (index, &track_id) in (0_u32..).zip(&job.track_ids) {
            if !self.wait_while_paused(&job.cancel) {
                return;
            }
            let before = index * STEPS_PER_TRACK;
            let path = lock(library).track(track_id).map(|detail| detail.path);
            let analyzed = path.and_then(|path: PathBuf| {
                // Report whole steps only: a few events per track, not one per call.
                let last = AtomicU64::new(u64::MAX);
                let sink = |fraction: f32| {
                    let step = (fraction.clamp(0.0, 1.0) * STEPS_PER_TRACK as f32) as u32;
                    if last.swap(u64::from(step), Ordering::Relaxed) != u64::from(step) {
                        report(before + step);
                    }
                };
                // A panic in a decoder or detector fails this track only.
                catch_unwind(AssertUnwindSafe(|| analyze(&path, &sink, &job.cancel))).unwrap_or(
                    Err(CoreError::InvalidAudio(
                        "the analysis of this track panicked",
                    )),
                )
            });
            match analyzed.and_then(|found| lock(library).store_analysis(track_id, &found)) {
                Ok(()) => {}
                Err(CoreError::Cancelled) => return,
                // Removed from the library while it waited: nothing to analyze.
                Err(CoreError::NotFound { .. }) => {}
                Err(err) => failures.push(err),
            }
        }
        if job.cancel.is_cancelled() {
            return;
        }
        let failed = failures.len();
        match failures.into_iter().next() {
            None => events.done(JobDone {
                job_id: job.id.clone(),
                kind: JobKind::Analysis,
            }),
            Some(first) => {
                let mut error = AppError::from(first);
                error.message = format!(
                    "{failed} of {count} tracks could not be analyzed; first: {}",
                    error.message
                );
                events.failed(JobFailed {
                    job_id: job.id.clone(),
                    kind: JobKind::Analysis,
                    error,
                });
            }
        }
    }
}

fn lock(library: &Mutex<Library>) -> MutexGuard<'_, Library> {
    // A poisoned lock still holds a usable connection (SQLite rolled back).
    library.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Queues an analysis of the tracks; returns the job id. Progress, done and
/// failure arrive as events. Rejected right away when the library could not
/// be opened.
#[tauri::command]
pub(crate) fn analyze_tracks(
    app: AppHandle,
    jobs: tauri::State<'_, Jobs>,
    library: tauri::State<'_, LibraryState>,
    ids: Vec<TrackId>,
) -> Result<String, AppError> {
    library.library()?;
    Ok(jobs.queue_analysis(ids, &app))
}

/// Cancels a job (queued or running). Unknown ids are ignored.
#[tauri::command]
pub(crate) fn job_cancel(jobs: tauri::State<'_, Jobs>, job_id: String) -> Result<(), AppError> {
    jobs.cancel(&job_id);
    Ok(())
}

/// Pauses or resumes the job queue.
#[tauri::command]
pub(crate) fn jobs_set_paused(jobs: tauri::State<'_, Jobs>, paused: bool) -> Result<(), AppError> {
    jobs.set_paused(paused);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Records events instead of sending them to a window.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Recorder {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    impl JobEvents for Recorder {
        fn progress(&self, event: JobProgress) {
            let line = if event.queued {
                format!("queued {} {}/{}", event.job_id, event.done, event.total)
            } else {
                format!("progress {}/{}", event.done, event.total)
            };
            self.0.lock().unwrap().push(line);
        }
        fn done(&self, event: JobDone) {
            self.0
                .lock()
                .unwrap()
                .push(format!("done {}", event.job_id));
        }
        fn failed(&self, event: JobFailed) {
            self.0.lock().unwrap().push(format!(
                "failed {} {}: {}",
                event.job_id, event.error.code, event.error.message
            ));
        }
    }

    /// Shares a recorder with the download thread.
    impl JobEvents for Arc<Recorder> {
        fn progress(&self, event: JobProgress) {
            self.as_ref().progress(event);
        }
        fn done(&self, event: JobDone) {
            self.as_ref().done(event);
        }
        fn failed(&self, event: JobFailed) {
            self.as_ref().failed(event);
        }
    }

    /// A one-file model fetched from `url` (leaked: tests only).
    fn model_at(url: String, size: u64) -> Model {
        let files = vec![cratekit_core::stems::models::ModelFile {
            name: "graph",
            url: Box::leak(url.into_boxed_str()),
            size,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        }];
        Model {
            name: "test-model",
            files: Box::leak(files.into_boxed_slice()),
        }
    }

    /// Waits until the download thread is done.
    fn wait_for_download(jobs: &Jobs) {
        for _ in 0..500 {
            if !jobs.is_downloading() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the download did not finish");
    }

    #[test]
    fn a_downloaded_model_finishes_without_the_network() {
        let dir = std::env::temp_dir().join(format!("cratekit-jobs-dl-{}", std::process::id()));
        let model = model_at("http://127.0.0.1:9/never".into(), 3);
        std::fs::create_dir_all(model.dir(&dir)).unwrap();
        std::fs::write(model.dir(&dir).join("graph"), b"abc").unwrap();
        let (jobs, recorder) = (Jobs::default(), Arc::new(Recorder::default()));

        let id = jobs.start_download(model, dir.clone(), Arc::clone(&recorder));
        wait_for_download(&jobs);

        let events = recorder.take();
        assert_eq!(events.first().map(String::as_str), Some("progress 0/1000"));
        assert_eq!(events.last(), Some(&format!("done {id}")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_download_is_reported_with_the_download_code() {
        // Nothing listens on this port: the connection is refused.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/graph", listener.local_addr().unwrap());
        drop(listener);
        let dir = std::env::temp_dir().join(format!("cratekit-jobs-fail-{}", std::process::id()));
        let (jobs, recorder) = (Jobs::default(), Arc::new(Recorder::default()));

        let id = jobs.start_download(model_at(url, 3), dir.clone(), Arc::clone(&recorder));
        wait_for_download(&jobs);

        let events = recorder.take();
        let last = events.last().expect("an event");
        assert!(
            last.starts_with(&format!("failed {id} download:")),
            "{last}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_second_download_request_joins_the_running_one() {
        // Accepts connections but never answers, so the download stays running.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/graph", listener.local_addr().unwrap());
        let dir = std::env::temp_dir().join(format!("cratekit-jobs-join-{}", std::process::id()));
        let (jobs, recorder) = (Jobs::default(), Arc::new(Recorder::default()));

        let first =
            jobs.start_download(model_at(url.clone(), 3), dir.clone(), Arc::clone(&recorder));
        let second = jobs.start_download(model_at(url, 3), dir.clone(), Arc::clone(&recorder));

        assert_eq!(first, second);
        assert!(jobs.is_downloading());
        drop(listener);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/cratekit-core/tests/fixtures")
            .join(name)
    }

    fn library_with(files: &[&str]) -> (Mutex<Library>, Vec<TrackId>) {
        let mut library = Library::open_in_memory().unwrap();
        let paths: Vec<PathBuf> = files.iter().map(|name| fixture(name)).collect();
        let ids = library.add_paths(&paths).unwrap().added;
        (Mutex::new(library), ids)
    }

    fn job(jobs: &Jobs, ids: Vec<TrackId>) -> Job {
        jobs.queue_analysis(ids, &Recorder::default());
        jobs.next()
    }

    #[test]
    fn analyzes_every_track_stores_the_results_and_reports_progress() {
        let (library, ids) = library_with(&["tone.wav", "tone.flac"]);
        let jobs = Jobs::default();
        let events = Recorder::default();
        let job = job(&jobs, ids.clone());
        jobs.run_analysis(&job, &library, &events, analyze_file);
        let seen = events.take();
        assert_eq!(seen.first().map(String::as_str), Some("progress 0/200"));
        assert_eq!(seen.last().map(String::as_str), Some("done job-1"));
        assert!(seen.contains(&"progress 100/200".to_owned()), "{seen:?}");
        assert!(seen.contains(&"progress 200/200".to_owned()), "{seen:?}");
        for id in ids {
            let detail = lock(&library).track(id).unwrap();
            assert!(detail.track.analyzed);
            assert!(detail.analysis.is_some_and(|a| a.lufs.is_some()));
        }
    }

    #[test]
    fn a_failing_track_is_reported_and_the_others_still_get_analyzed() {
        // The middle track's file is deleted after it was added.
        let scratch = std::env::temp_dir().join(format!("cratekit-jobs-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let doomed = scratch.join("deleted later.wav");
        std::fs::copy(fixture("mono-48k.wav"), &doomed).unwrap();
        let (library, first) = library_with(&["tone.wav"]);
        let middle = lock(&library).add_paths(&[doomed]).unwrap().added;
        let last = lock(&library)
            .add_paths(&[fixture("tone.mp3")])
            .unwrap()
            .added;
        std::fs::remove_dir_all(&scratch).unwrap();
        let ids = vec![first[0], middle[0], last[0]];

        let jobs = Jobs::default();
        let events = Recorder::default();
        let job = job(&jobs, ids.clone());
        jobs.run_analysis(&job, &library, &events, analyze_file);
        let outcome = events.take().pop().unwrap();
        assert!(
            outcome.starts_with("failed job-1 io: 1 of 3 tracks could not be analyzed"),
            "{outcome}"
        );
        assert!(lock(&library).track(ids[0]).unwrap().track.analyzed);
        assert!(!lock(&library).track(ids[1]).unwrap().track.analyzed);
        assert!(lock(&library).track(ids[2]).unwrap().track.analyzed);
    }

    #[test]
    fn a_panicking_analysis_fails_its_track_only() {
        fn panics_on_flac(
            path: &Path,
            progress: &dyn ProgressSink,
            cancel: &CancellationToken,
        ) -> Result<TrackAnalysis, CoreError> {
            assert!(
                path.extension().is_none_or(|ext| ext != "flac"),
                "a decoder bug"
            );
            analyze_file(path, progress, cancel)
        }
        let (library, ids) = library_with(&["tone.flac", "tone.wav"]);
        let jobs = Jobs::default();
        let events = Recorder::default();
        let job = job(&jobs, ids.clone());
        jobs.run_analysis(&job, &library, &events, panics_on_flac);
        let outcome = events.take().pop().unwrap();
        assert!(outcome.contains("1 of 2 tracks"), "{outcome}");
        assert!(lock(&library).track(ids[1]).unwrap().track.analyzed);
    }

    #[test]
    fn a_cancelled_job_stops_and_sends_nothing_more() {
        let (library, ids) = library_with(&["tone.wav", "tone.flac"]);
        let jobs = Jobs::default();
        let events = Recorder::default();
        let job = job(&jobs, ids.clone());
        jobs.cancel(&job.id);
        jobs.run_analysis(&job, &library, &events, analyze_file);
        assert_eq!(events.take(), ["progress 0/200"]);
        assert!(!lock(&library).track(ids[0]).unwrap().track.analyzed);
    }

    #[test]
    fn cancelling_a_waiting_job_removes_it_and_pausing_holds_new_jobs() {
        let jobs = Jobs::default();
        let events = Recorder::default();
        let first = jobs.queue_analysis(vec![1], &events);
        let second = jobs.queue_analysis(vec![2], &events);
        assert_eq!((first.as_str(), second.as_str()), ("job-1", "job-2"));
        assert_eq!(
            events.take(),
            ["queued job-1 0/100", "queued job-2 0/100"],
            "the UI hears of each job as soon as it is queued"
        );
        jobs.cancel(&first);
        jobs.set_paused(true);
        let waiting = jobs.clone();
        let worker = std::thread::spawn(move || waiting.next().id);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!worker.is_finished(), "a paused queue starts nothing");
        jobs.set_paused(false);
        assert_eq!(worker.join().unwrap(), "job-2");
    }

    #[test]
    fn cancelling_a_running_job_wakes_it_while_the_queue_is_paused() {
        let jobs = Jobs::default();
        let running = job(&jobs, vec![1]);
        jobs.set_paused(true);
        let waiter = jobs.clone();
        let token = running.cancel.clone();
        let worker = std::thread::spawn(move || waiter.wait_while_paused(&token));
        std::thread::sleep(std::time::Duration::from_millis(50));
        jobs.cancel(&running.id);
        assert!(!worker.join().unwrap(), "it stops without a resume");
    }

    #[test]
    fn events_serialize_as_the_ui_types() {
        let progress = JobProgress {
            job_id: "job-3".into(),
            kind: JobKind::Analysis,
            label: "Analyzing 2 tracks".into(),
            stage: None,
            track_ids: vec![4, 5],
            done: 150,
            total: 200,
            queued: false,
        };
        assert_eq!(
            serde_json::to_value(progress).unwrap(),
            serde_json::json!({
                "jobId": "job-3", "kind": "analysis", "label": "Analyzing 2 tracks",
                "stage": null, "trackIds": [4, 5], "done": 150, "total": 200, "queued": false
            })
        );
        let failed = JobFailed {
            job_id: "job-3".into(),
            kind: JobKind::Analysis,
            error: AppError {
                code: "io",
                message: "x".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(failed).unwrap(),
            serde_json::json!({
                "jobId": "job-3", "kind": "analysis", "error": { "code": "io", "message": "x" }
            })
        );
        let download = JobProgress::model_download("job-4", "htdemucs", Stage::Verify, 0.5);
        assert_eq!(
            serde_json::to_value(download).unwrap(),
            serde_json::json!({
                "jobId": "job-4", "kind": "model-download", "label": "Verifying htdemucs checksum",
                "stage": "verify", "trackIds": [], "done": 500, "total": 1000, "queued": false
            })
        );
    }
}
