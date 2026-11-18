//! Progress reporting and cancellation for long jobs (analysis, stems,
//! mastering).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::CoreError;

/// Receives how far a long task has got, as a fraction from 0 to 1. Called
/// from the worker thread, a few times per task (not per sample), so it can
/// send an event or update a counter.
pub trait ProgressSink: Send + Sync {
    /// The task is `fraction` (0 to 1) done.
    fn progress(&self, fraction: f32);
}

/// Any `Fn(f32)` closure is a progress sink.
impl<F: Fn(f32) + Send + Sync> ProgressSink for F {
    fn progress(&self, fraction: f32) {
        self(fraction);
    }
}

/// A progress sink that ignores everything (for callers that do not show
/// progress).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn progress(&self, _fraction: f32) {}
}

/// Asks a running task to stop. Clones share one flag: cancel any clone and
/// every clone sees it. Tasks check it between steps and stop with
/// [`CoreError::Cancelled`].
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// A token that is not cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks every task holding a clone of this token to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether [`cancel`](Self::cancel) was called on any clone.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Stops the task: `Err(CoreError::Cancelled)` once cancelled, else `Ok`.
    /// Tasks call it between steps with `?`.
    ///
    /// # Errors
    ///
    /// [`CoreError::Cancelled`] when the token was cancelled.
    pub fn check(&self) -> Result<(), CoreError> {
        if self.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn cancelling_any_clone_cancels_them_all() {
        let token = CancellationToken::new();
        let worker = token.clone();
        assert!(worker.check().is_ok());
        token.cancel();
        assert!(worker.is_cancelled());
        assert!(matches!(worker.check(), Err(CoreError::Cancelled)));
    }

    #[test]
    fn closures_are_progress_sinks() {
        let seen = Mutex::new(Vec::new());
        let sink = |fraction: f32| seen.lock().expect("lock").push(fraction);
        sink.progress(0.25);
        ProgressSink::progress(&sink, 1.0);
        NoProgress.progress(0.5);
        assert_eq!(*seen.lock().expect("lock"), [0.25, 1.0]);
    }
}
