//! The background jobs: analysis and stems jobs run one at a time on a
//! worker thread and can be paused; the model download and master exports
//! run beside the queue, on their own threads. All report progress as events
//! and can be cancelled.
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
use cratekit_core::audio::{WavFormat, decode_file_with, write_wav};
use cratekit_core::library::{Library, TrackId};
use cratekit_core::mastering::master::{MasterSettings, Mastered, master};
use cratekit_core::progress::{CancellationToken, ProgressSink};
use cratekit_core::stems::StemSeparator;
use cratekit_core::stems::cache::split_to_folder;
use cratekit_core::stems::models::{HTDEMUCS, Model, Stage};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppError;
use crate::library::{LibraryState, lock};

/// Steps of progress per track in `job_progress` (`done` and `total` count
/// these, so a single track still moves smoothly).
const STEPS_PER_TRACK: u32 = 100;

/// What a job does (the UI's `JobKind`). The VirtualDJ export adds its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum JobKind {
    /// Tempo, key, loudness and energy of tracks.
    Analysis,
    /// Splitting tracks into stems.
    Stems,
    /// Mastering a track into a WAV file.
    Master,
    /// The stems model download.
    ModelDownload,
}

/// Shares of a master export's progress: decoding the original, then
/// mastering it; writing the file is the rest.
const MASTER_DECODE_SHARE: f32 = 0.1;
const MASTER_RENDER_SHARE: f32 = 0.85;

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
        Self::for_tracks(JobKind::Analysis, job_id, track_ids, done, queued)
    }

    /// The progress event of a job over tracks (analysis or stems).
    fn for_tracks(
        kind: JobKind,
        job_id: &str,
        track_ids: &[TrackId],
        done: u32,
        queued: bool,
    ) -> Self {
        let count = u32::try_from(track_ids.len()).unwrap_or(u32::MAX);
        let label = match kind {
            JobKind::Stems => format!("Splitting {count} tracks into stems"),
            JobKind::Master => "Mastering a track".to_owned(),
            _ => format!("Analyzing {count} tracks"),
        };
        Self {
            job_id: job_id.to_owned(),
            kind,
            label,
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
    kind: JobKind,
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
    /// Jobs running beside the queue on their callers' threads (master
    /// exports): their ids and how to cancel them.
    side: Vec<(String, CancellationToken)>,
}

/// What a master export renders: the track, the file it goes to, how it is
/// mastered and what the library records with it.
#[derive(Debug, Clone)]
pub(crate) struct MasterRequest {
    pub(crate) track_id: TrackId,
    /// The original file, only read.
    pub(crate) source: PathBuf,
    /// The WAV file to write.
    pub(crate) file: PathBuf,
    pub(crate) settings: MasterSettings,
    pub(crate) format: WavFormat,
    /// The settings as JSON, recorded with the master.
    pub(crate) recorded: String,
}

/// A job running beside the queue (a master export). While it lives,
/// `job_cancel` reaches it; dropping it takes it off the list however the
/// job ends, a panic included.
struct SideJob<'a> {
    jobs: &'a Jobs,
    id: String,
    cancel: CancellationToken,
}

impl Drop for SideJob<'_> {
    fn drop(&mut self) {
        self.jobs.lock().side.retain(|(id, _)| *id != self.id);
    }
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

    /// A new job id: `job-1`, `job-2`, and so on.
    fn next_id(&self) -> String {
        format!("job-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Registers a job that runs beside the queue, so `job_cancel` reaches
    /// it until the returned guard is dropped.
    fn start_side(&self) -> SideJob<'_> {
        let id = self.next_id();
        let cancel = CancellationToken::new();
        self.lock().side.push((id.clone(), cancel.clone()));
        SideJob {
            jobs: self,
            id,
            cancel,
        }
    }

    /// Queues an analysis of `track_ids` and returns its job id. Tells the
    /// UI right away (a queued `job_progress`), before the worker can start
    /// it, so a waiting job can be seen and cancelled.
    pub(crate) fn queue_analysis(&self, track_ids: Vec<TrackId>, events: &dyn JobEvents) -> String {
        self.queue(JobKind::Analysis, track_ids, events)
    }

    /// Queues an analysis of tracks just added to the library, so they are
    /// analyzed in the background after an import (SPEC Module 1); returns
    /// the job id, or `None` when nothing was added.
    pub(crate) fn analyze_added(
        &self,
        added: &[TrackId],
        events: &dyn JobEvents,
    ) -> Option<String> {
        if added.is_empty() {
            return None;
        }
        Some(self.queue_analysis(added.to_vec(), events))
    }

    /// Queues a job of `kind` over `track_ids` (analysis or stems) and
    /// returns its id, telling the UI right away, as [`Self::queue_analysis`].
    pub(crate) fn queue(
        &self,
        kind: JobKind,
        track_ids: Vec<TrackId>,
        events: &dyn JobEvents,
    ) -> String {
        let id = self.next_id();
        events.progress(JobProgress::for_tracks(kind, &id, &track_ids, 0, true));
        self.lock().waiting.push_back(Job {
            id: id.clone(),
            kind,
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
        for (_, cancel) in queue.side.iter().filter(|(id, _)| id == job_id) {
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
            let id = self.next_id();
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
                        Ok(library) => match job.kind {
                            JobKind::Stems => match crate::stems::stems_dir(&app) {
                                Ok(root) => {
                                    jobs.run_stems(&job, library, &app, &root, &|| {
                                        crate::stems::separator(&app)
                                    });
                                    crate::playback::stems_split_done(&app, &job.track_ids);
                                }
                                Err(error) => app.failed(JobFailed {
                                    job_id: job.id.clone(),
                                    kind: job.kind,
                                    error,
                                }),
                            },
                            JobKind::Analysis => {
                                jobs.run_analysis(&job, library, &app, analyze_file);
                            }
                            // These run beside the queue and are never queued.
                            JobKind::Master | JobKind::ModelDownload => app.failed(JobFailed {
                                job_id: job.id.clone(),
                                kind: job.kind,
                                error: AppError {
                                    code: "internal",
                                    message: "this job does not run on the queue".into(),
                                },
                            }),
                        },
                        Err(error) => app.failed(JobFailed {
                            job_id: job.id.clone(),
                            kind: job.kind,
                            error,
                        }),
                    }
                }));
                if outcome.is_err() {
                    app.failed(JobFailed {
                        job_id: job.id.clone(),
                        kind: job.kind,
                        error: AppError {
                            code: "internal",
                            message: "the job stopped unexpectedly".into(),
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
        finish_tracks(job, failures, count, "analyzed", events);
    }

    /// Splits every track of `job` into stems, writes them into
    /// `<stems_root>/<track id>/` and records them in the library. The
    /// separator (ONNX Runtime and the model) is made once per job by
    /// `make_separator`; if that fails, the job fails with its reason. Tracks
    /// that fail are skipped, as in an analysis.
    fn run_stems(
        &self,
        job: &Job,
        library: &Mutex<Library>,
        events: &dyn JobEvents,
        stems_root: &Path,
        make_separator: &dyn Fn() -> Result<Box<dyn StemSeparator>, AppError>,
    ) {
        let count = u32::try_from(job.track_ids.len()).unwrap_or(u32::MAX);
        let report = |done: u32| {
            events.progress(JobProgress::for_tracks(
                JobKind::Stems,
                &job.id,
                &job.track_ids,
                done,
                false,
            ));
        };
        report(0);
        let mut separator = match make_separator() {
            Ok(separator) => separator,
            Err(error) => {
                events.failed(JobFailed {
                    job_id: job.id.clone(),
                    kind: JobKind::Stems,
                    error,
                });
                return;
            }
        };
        let mut failures: Vec<CoreError> = Vec::new();
        for (index, &track_id) in (0_u32..).zip(&job.track_ids) {
            if !self.wait_while_paused(&job.cancel) {
                return;
            }
            let before = index * STEPS_PER_TRACK;
            let dir = stems_root.join(track_id.to_string());
            let path = match lock(library).track(track_id) {
                Ok(detail) => detail.path,
                // Removed from the library while it waited: nothing to split.
                Err(CoreError::NotFound { .. }) => continue,
                Err(err) => {
                    failures.push(err);
                    continue;
                }
            };
            let last = AtomicU64::new(u64::MAX);
            let sink = |fraction: f32| {
                let step = (fraction.clamp(0.0, 1.0) * STEPS_PER_TRACK as f32) as u32;
                if last.swap(u64::from(step), Ordering::Relaxed) != u64::from(step) {
                    report(before + step);
                }
            };
            // A panic in the decoder or the model fails this track only.
            let split = catch_unwind(AssertUnwindSafe(|| {
                split_to_folder(separator.as_mut(), &path, &dir, &sink, &job.cancel)
            }))
            .unwrap_or(Err(CoreError::Stems(
                "splitting this track panicked".to_owned(),
            )));
            let stored = split.and_then(|split| {
                let mut library = lock(library);
                // The track may have been removed while it was split, and its
                // id given to a new track: then these stems are not its own.
                if library
                    .track(track_id)
                    .is_ok_and(|detail| detail.path == path)
                {
                    library.store_stems(track_id, HTDEMUCS.name, &dir, &split)
                } else {
                    drop(library);
                    std::fs::remove_dir_all(&dir).ok();
                    Err(CoreError::NotFound {
                        what: "track",
                        id: track_id,
                    })
                }
            });
            match stored {
                Ok(()) => {}
                Err(CoreError::Cancelled) => return,
                Err(CoreError::NotFound { .. }) => {}
                Err(err) => failures.push(err),
            }
        }
        finish_tracks(job, failures, count, "split", events);
    }

    /// Masters a track as a job that starts at once, beside the queue: an
    /// export the user waits for never waits behind an import's analysis,
    /// and pausing the queue does not hold it. Decodes the original, masters
    /// it, writes `request.file` and records it in the library, reporting
    /// progress as `job_progress` (kind `master`). `job_cancel` stops it
    /// until the file starts being written. Ends with `job_done`, or with
    /// `job_failed` carrying the error it also returns; a cancelled export
    /// returns `cancelled` and sends nothing more.
    pub(crate) fn run_master(
        &self,
        request: &MasterRequest,
        library: &Mutex<Library>,
        events: &dyn JobEvents,
    ) -> Result<Mastered, AppError> {
        let job = self.start_side();
        let track_ids = [request.track_id];
        let report = |done: u32| {
            events.progress(JobProgress::for_tracks(
                JobKind::Master,
                &job.id,
                &track_ids,
                done,
                false,
            ));
        };
        report(0);
        // Report whole steps only, as an analysis does.
        let last = AtomicU64::new(0);
        let step = |fraction: f32| {
            let step = (fraction.clamp(0.0, 1.0) * STEPS_PER_TRACK as f32) as u32;
            if last.swap(u64::from(step), Ordering::Relaxed) != u64::from(step) {
                report(step);
            }
        };
        let decoding = |fraction: f32| step(MASTER_DECODE_SHARE * fraction);
        let mastering = |fraction: f32| step(MASTER_DECODE_SHARE + MASTER_RENDER_SHARE * fraction);
        // A panic in a decoder or the DSP fails this export, not the app.
        let rendered = catch_unwind(AssertUnwindSafe(|| {
            let audio = decode_file_with(&request.source, &decoding, &job.cancel)?;
            let mastered = master(audio, &request.settings, &mastering, &job.cancel)?;
            // The last chance to cancel: a file being written is finished.
            job.cancel.check()?;
            Ok(mastered)
        }))
        .unwrap_or(Err(CoreError::InvalidAudio(
            "mastering this track stopped unexpectedly",
        )));
        let outcome = match rendered {
            Err(CoreError::Cancelled) => return Err(CoreError::Cancelled.into()),
            Err(err) => Err(err.into()),
            // Guarded too: a job must end with an event, or the UI would
            // show it running forever.
            Ok(mastered) => catch_unwind(AssertUnwindSafe(|| {
                write_master(request, library, &mastered)
            }))
            .unwrap_or_else(|_| {
                Err(AppError {
                    code: "internal",
                    message: "writing the master stopped unexpectedly".into(),
                })
            })
            .map(|()| mastered),
        };
        match &outcome {
            Ok(_) => events.done(JobDone {
                job_id: job.id.clone(),
                kind: JobKind::Master,
            }),
            Err(error) => events.failed(JobFailed {
                job_id: job.id.clone(),
                kind: JobKind::Master,
                error: error.clone(),
            }),
        }
        outcome
    }
}

/// Writes a master to its file and records it in the library (the track
/// then shows as mastered). The track may have been removed while it was
/// mastered, and its id given to a new track: then, as for stems, nothing is
/// written or recorded and the export fails with `not_found`.
fn write_master(
    request: &MasterRequest,
    library: &Mutex<Library>,
    mastered: &Mastered,
) -> Result<(), AppError> {
    let still_there = |library: &Library| {
        library
            .track(request.track_id)
            .is_ok_and(|detail| detail.path == request.source)
    };
    let gone = || {
        AppError::from(CoreError::NotFound {
            what: "track",
            id: request.track_id,
        })
    };
    if !still_there(&lock(library)) {
        return Err(gone());
    }
    write_wav(&request.file, &mastered.audio, request.format).map_err(|err| AppError {
        code: "io",
        message: format!("{}: {err}", request.file.display()),
    })?;
    // Checked again under the lock that records it: removed while writing.
    let mut library = lock(library);
    if !still_there(&library) {
        drop(library);
        std::fs::remove_file(&request.file).ok();
        return Err(gone());
    }
    library.store_master(
        request.track_id,
        request.settings.preset.id(),
        &request.recorded,
        &request.file,
        mastered.lufs,
    )?;
    Ok(())
}

/// Ends a job over tracks: `job_done`, or `job_failed` saying how many tracks
/// failed and the first reason. A cancelled job sends nothing.
fn finish_tracks(
    job: &Job,
    failures: Vec<CoreError>,
    count: u32,
    verb: &str,
    events: &dyn JobEvents,
) {
    if job.cancel.is_cancelled() {
        return;
    }
    let failed = failures.len();
    match failures.into_iter().next() {
        None => events.done(JobDone {
            job_id: job.id.clone(),
            kind: job.kind,
        }),
        Some(first) => {
            let mut error = AppError::from(first);
            error.message = format!(
                "{failed} of {count} tracks could not be {verb}; first: {}",
                error.message
            );
            events.failed(JobFailed {
                job_id: job.id.clone(),
                kind: job.kind,
                error,
            });
        }
    }
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

    use cratekit_core::mastering::presets::PresetName;

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

    /// A stand-in for the model: drums get half the mix, bass and other a
    /// quarter each, vocals nothing.
    struct Halves;

    impl StemSeparator for Halves {
        fn separate(
            &mut self,
            audio: cratekit_core::audio::AudioBuffer,
            progress: &dyn ProgressSink,
            cancel: &CancellationToken,
        ) -> Result<cratekit_core::stems::Stems, CoreError> {
            cancel.check()?;
            progress.progress(1.0);
            let scaled = |k: f32| {
                cratekit_core::audio::AudioBuffer::new(
                    audio.sample_rate(),
                    audio
                        .channels()
                        .iter()
                        .map(|c| c.iter().map(|s| s * k).collect())
                        .collect(),
                )
            };
            Ok(cratekit_core::stems::Stems {
                drums: scaled(0.5)?,
                bass: scaled(0.25)?,
                other: scaled(0.25)?,
                vocals: scaled(0.0)?,
            })
        }
    }

    fn stems_job(jobs: &Jobs, ids: Vec<TrackId>) -> Job {
        jobs.queue(JobKind::Stems, ids, &Recorder::default());
        jobs.next()
    }

    #[test]
    fn splits_every_track_into_the_cache_and_records_it() {
        let (library, ids) = library_with(&["tone.wav", "tone.flac"]);
        let root = std::env::temp_dir().join(format!("cratekit-jobs-stems-{}", std::process::id()));
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let job = stems_job(&jobs, ids.clone());

        jobs.run_stems(&job, &library, &events, &root, &|| Ok(Box::new(Halves)));

        let seen = events.take();
        assert_eq!(seen.first().map(String::as_str), Some("progress 0/200"));
        assert_eq!(seen.last().map(String::as_str), Some("done job-1"));
        for id in ids {
            let dir = root.join(id.to_string());
            for file in cratekit_core::stems::cache::STEM_FILES {
                assert!(dir.join(file).is_file(), "{file}");
            }
            let stored = lock(&library).stems(id).unwrap().expect("recorded");
            assert_eq!(stored.dir, dir);
            assert!(stored.waveforms.is_some());
            assert!(lock(&library).track(id).unwrap().track.has_stems);
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// Removes the track from the library while "splitting" it, as a user
    /// could during a long split.
    struct RemovesTrack(Arc<Mutex<Library>>, TrackId);

    impl StemSeparator for RemovesTrack {
        fn separate(
            &mut self,
            audio: cratekit_core::audio::AudioBuffer,
            progress: &dyn ProgressSink,
            cancel: &CancellationToken,
        ) -> Result<cratekit_core::stems::Stems, CoreError> {
            lock(&self.0).remove_tracks(&[self.1]).unwrap();
            Halves.separate(audio, progress, cancel)
        }
    }

    #[test]
    fn a_track_removed_during_its_split_keeps_no_stems() {
        let (library, ids) = library_with(&["tone.wav"]);
        let library = Arc::new(library);
        let root =
            std::env::temp_dir().join(format!("cratekit-jobs-removed-{}", std::process::id()));
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let job = stems_job(&jobs, ids.clone());
        let id = ids[0];
        let shared = Arc::clone(&library);
        let make = move || -> Result<Box<dyn StemSeparator>, AppError> {
            Ok(Box::new(RemovesTrack(Arc::clone(&shared), id)))
        };

        jobs.run_stems(&job, &library, &events, &root, &make);

        assert_eq!(events.take().last().map(String::as_str), Some("done job-1"));
        assert!(
            !root.join(id.to_string()).exists(),
            "the stems folder is removed"
        );
        assert!(lock(&library).stems(id).unwrap().is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_separator_that_cannot_be_made_fails_the_job_with_its_reason() {
        let (library, ids) = library_with(&["tone.wav"]);
        let root =
            std::env::temp_dir().join(format!("cratekit-jobs-nostems-{}", std::process::id()));
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let job = stems_job(&jobs, ids.clone());

        jobs.run_stems(&job, &library, &events, &root, &|| {
            Err(AppError {
                code: "stems_unavailable",
                message: "stems need a newer CPU".into(),
            })
        });

        let seen = events.take();
        assert_eq!(
            seen.last().map(String::as_str),
            Some("failed job-1 stems_unavailable: stems need a newer CPU")
        );
        assert!(lock(&library).stems(ids[0]).unwrap().is_none());
        assert!(!root.exists());
    }

    #[test]
    fn a_cancelled_stems_job_sends_nothing_more_and_records_nothing() {
        let (library, ids) = library_with(&["tone.wav"]);
        let root =
            std::env::temp_dir().join(format!("cratekit-jobs-cancel-{}", std::process::id()));
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let job = stems_job(&jobs, ids.clone());
        job.cancel.cancel();

        jobs.run_stems(&job, &library, &events, &root, &|| Ok(Box::new(Halves)));

        assert_eq!(events.take(), vec!["progress 0/100".to_owned()]);
        assert!(lock(&library).stems(ids[0]).unwrap().is_none());
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
    fn an_import_queues_one_analysis_of_the_stored_tracks() {
        let library = Mutex::new(Library::open_in_memory().unwrap());
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let paths = [fixture("tone.mp3"), fixture("tone.flac")];

        let added = crate::library::add_paths(&library, &jobs, &events, &paths).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(events.take(), ["queued job-1 0/200"]);
        let job = jobs.next();
        assert_eq!(job.kind, JobKind::Analysis);
        assert_eq!(job.track_ids, added);
        // Queued after the store: the worker finds every track.
        for id in &added {
            assert!(lock(&library).track(*id).is_ok());
        }

        // The same files again: nothing new, nothing queued.
        let again = crate::library::add_paths(&library, &jobs, &events, &paths).unwrap();
        assert!(again.is_empty());
        assert!(events.take().is_empty());
    }

    #[test]
    fn an_import_analyzes_only_the_files_it_could_add() {
        let dir = std::env::temp_dir().join(format!("cratekit-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("not audio.mp3");
        std::fs::write(&fake, b"this is not an mp3").unwrap();
        let library = Mutex::new(Library::open_in_memory().unwrap());
        let (jobs, events) = (Jobs::default(), Recorder::default());

        // Only a bad file: its error, and no job.
        let failed =
            crate::library::add_paths(&library, &jobs, &events, std::slice::from_ref(&fake));
        assert!(failed.is_err());
        assert!(events.take().is_empty());

        // A good and a bad file: the good one is added and analyzed.
        let added =
            crate::library::add_paths(&library, &jobs, &events, &[fake, fixture("tone.mp3")])
                .unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(events.take(), ["queued job-1 0/100"]);
        assert_eq!(jobs.next().track_ids, added);
        std::fs::remove_dir_all(&dir).ok();
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

    /// A master export of track `id` into `dir/master.wav`.
    fn master_request(library: &Mutex<Library>, id: TrackId, dir: &Path) -> MasterRequest {
        MasterRequest {
            track_id: id,
            source: lock(library).track(id).unwrap().path,
            file: dir.join("master.wav"),
            settings: MasterSettings {
                preset: PresetName::Club,
                intensity: 0.6,
                target_lufs: -10.0,
            },
            format: WavFormat::Pcm24,
            recorded: "{}".into(),
        }
    }

    /// Cancels a job once its progress reaches a step, as the Cancel button
    /// does, and records the events.
    struct CancelsAt<'a>(&'a Jobs, u32, Recorder);

    impl JobEvents for CancelsAt<'_> {
        fn progress(&self, event: JobProgress) {
            if event.done >= self.1 {
                self.0.cancel(&event.job_id);
            }
            self.2.progress(event);
        }
        fn done(&self, event: JobDone) {
            self.2.done(event);
        }
        fn failed(&self, event: JobFailed) {
            self.2.failed(event);
        }
    }

    /// Removes the track as soon as its export starts and adds another file,
    /// which gets the same id back, as a user could during a long export.
    struct ReplacesTrack<'a>(&'a Mutex<Library>, TrackId, Recorder);

    impl JobEvents for ReplacesTrack<'_> {
        fn progress(&self, event: JobProgress) {
            if event.done == 0 {
                let mut library = lock(self.0);
                library.remove_tracks(&[self.1]).unwrap();
                let added = library.add_paths(&[fixture("tone.flac")]).unwrap().added;
                assert_eq!(added, vec![self.1], "SQLite gives the id back");
            }
            self.2.progress(event);
        }
        fn done(&self, event: JobDone) {
            self.2.done(event);
        }
        fn failed(&self, event: JobFailed) {
            self.2.failed(event);
        }
    }

    /// The `done` steps of the recorded progress events.
    fn steps(events: &[String]) -> Vec<u32> {
        events
            .iter()
            .filter_map(|line| {
                line.strip_prefix("progress ")?
                    .split('/')
                    .next()?
                    .parse()
                    .ok()
            })
            .collect()
    }

    #[test]
    fn masters_a_track_into_its_file_records_it_and_reports_progress() {
        let (library, ids) = library_with(&["tone.wav"]);
        let dir = std::env::temp_dir().join(format!("cratekit-jobs-master-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let request = master_request(&library, ids[0], &dir);

        let mastered = jobs.run_master(&request, &library, &events).unwrap();

        let seen = events.take();
        assert_eq!(seen.first().map(String::as_str), Some("progress 0/100"));
        assert_eq!(seen.last().map(String::as_str), Some("done job-1"));
        let steps = steps(&seen);
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]), "{steps:?}");
        // Never 100 before the file is written and recorded: `job_done` then
        // ends the job.
        assert!(
            steps.last().is_some_and(|&last| (90..100).contains(&last)),
            "{steps:?}"
        );
        assert!(request.file.is_file());
        assert!(mastered.true_peak_dbtp <= -1.0);
        assert!(lock(&library).track(ids[0]).unwrap().track.mastered);
        assert!(
            jobs.lock().side.is_empty(),
            "a finished export takes no cancel"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cancelled_master_writes_nothing_and_sends_nothing_more() {
        // Cancelled at the start (decoding stops), and after mastering, just
        // before the file would be written (the last check stops it).
        for at in [0, 90] {
            let (library, ids) = library_with(&["tone.wav"]);
            let dir = std::env::temp_dir().join(format!(
                "cratekit-jobs-master-cancel-{at}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let jobs = Jobs::default();
            let events = CancelsAt(&jobs, at, Recorder::default());
            let request = master_request(&library, ids[0], &dir);

            let outcome = jobs.run_master(&request, &library, &events);

            assert_eq!(
                outcome.map(|_| ()).map_err(|err| err.code),
                Err("cancelled"),
                "at {at}"
            );
            let seen = events.2.take();
            assert_eq!(seen.first().map(String::as_str), Some("progress 0/100"));
            assert!(
                seen.iter().all(|line| line.starts_with("progress ")),
                "{seen:?}"
            );
            assert!(!request.file.exists(), "at {at}");
            assert!(!lock(&library).track(ids[0]).unwrap().track.mastered);
            assert!(jobs.lock().side.is_empty());
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn a_track_replaced_during_its_export_gets_no_master() {
        let (library, ids) = library_with(&["tone.wav"]);
        let dir = std::env::temp_dir().join(format!(
            "cratekit-jobs-master-replaced-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let jobs = Jobs::default();
        let events = ReplacesTrack(&library, ids[0], Recorder::default());
        let request = master_request(&library, ids[0], &dir);

        let error = jobs
            .run_master(&request, &library, &events)
            .expect_err("fails");

        assert_eq!(error.code, "not_found");
        let last = events.2.take().pop().expect("an event");
        assert!(last.starts_with("failed job-1 not_found:"), "{last}");
        assert!(!request.file.exists());
        assert!(
            !lock(&library).track(ids[0]).unwrap().track.mastered,
            "the new track with the same id is not marked"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_master_that_cannot_be_written_fails_naming_the_file() {
        let (library, ids) = library_with(&["tone.wav"]);
        let missing = std::env::temp_dir()
            .join(format!("cratekit-jobs-master-gone-{}", std::process::id()))
            .join("no such folder");
        let (jobs, events) = (Jobs::default(), Recorder::default());
        let request = master_request(&library, ids[0], &missing);

        let error = jobs
            .run_master(&request, &library, &events)
            .expect_err("fails");

        assert_eq!(error.code, "io");
        let file = request.file.display().to_string();
        assert!(error.message.starts_with(&file), "{}", error.message);
        let last = events.take().pop().expect("an event");
        assert!(
            last.starts_with(&format!("failed job-1 io: {file}")),
            "{last}"
        );
        assert!(!lock(&library).track(ids[0]).unwrap().track.mastered);
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
        let master = JobDone {
            job_id: "job-5".into(),
            kind: JobKind::Master,
        };
        assert_eq!(
            serde_json::to_value(master).unwrap(),
            serde_json::json!({ "jobId": "job-5", "kind": "master" })
        );
    }
}
