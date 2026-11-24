use serde::Serialize;

/// Pipeline stage, reported to callers for progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Queued,
    Extracting,
    DescribingImages,
    Classifying,
    Annotating,
    Done,
    Failed,
    Cancelled,
}

impl Stage {
    /// No further progress will be reported.
    pub fn is_finished(self) -> bool {
        matches!(self, Stage::Done | Stage::Failed | Stage::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub stage: Stage,
    pub done: usize,
    pub total: usize,
}

impl Progress {
    pub fn new(stage: Stage, done: usize, total: usize) -> Self {
        Self { stage, done, total }
    }

    pub fn stage(stage: Stage) -> Self {
        Self::new(stage, 0, 0)
    }
}

/// Progress callback. Must be cheap; called from the scoring task.
pub type ProgressFn<'a> = &'a (dyn Fn(Progress) + Send + Sync);

/// A callback that ignores progress.
pub fn no_progress(_: Progress) {}
