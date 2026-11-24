//! In-memory PDF jobs: upload -> background scoring -> poll -> download.
//! Jobs run one at a time (the LLM server is the bottleneck); the store
//! keeps the most recent `MAX_JOBS` and forgets them on restart. An
//! unfinished job can be cancelled: its task is aborted, which drops any
//! in-flight LLM request.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use scorer_engine::{Progress, ScoreReport, Scorer, Stage};
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::AbortHandle;

use crate::pdf;

const MAX_JOBS: usize = 30;

#[derive(Clone, Serialize)]
pub struct JobView {
    pub id: String,
    pub file_name: String,
    /// `provider/model` doing the scoring.
    pub model: String,
    pub progress: Progress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Arc<ScoreReport>>,
}

/// Why a job could not be cancelled.
pub enum CancelError {
    NotFound,
    Finished(Stage),
}

struct Job {
    view: JobView,
    annotated: Option<Arc<Vec<u8>>>,
    task: Option<AbortHandle>,
}

#[derive(Default)]
struct Inner {
    jobs: HashMap<String, Job>,
    order: VecDeque<String>,
}

pub struct JobStore {
    inner: Mutex<Inner>,
    runner: Semaphore,
    counter: AtomicU64,
}

impl Default for JobStore {
    fn default() -> Self {
        Self { inner: Mutex::default(), runner: Semaphore::new(1), counter: AtomicU64::new(0) }
    }
}

impl JobStore {
    /// Registers a job and starts it in the background. Returns its id.
    pub fn submit(self: &Arc<Self>, scorer: Arc<Scorer>, pdf: Vec<u8>, file_name: String) -> String {
        let id = self.new_id();
        // Hold the lock until the abort handle is stored, so a cancel cannot
        // slip in between spawning and registering.
        let mut inner = self.inner.lock().unwrap();
        evict_finished(&mut inner);

        let store = self.clone();
        let job_id = id.clone();
        let model = scorer.provider_info().model;
        let name = file_name.clone();
        let task = tokio::spawn(async move {
            let _permit = store.runner.acquire().await.expect("semaphore never closed");
            let on_progress = |p: Progress| store.update_live(&job_id, |j| j.view.progress = p);
            match pdf::score_pdf(&scorer, Arc::new(pdf), &name, &on_progress).await {
                Ok(outcome) => store.update_live(&job_id, |j| {
                    j.view.progress = Progress::stage(Stage::Done);
                    j.view.report = Some(Arc::new(outcome.report));
                    j.annotated = Some(Arc::new(outcome.annotated));
                    j.task = None;
                }),
                Err(e) => {
                    tracing::error!(job = %job_id, error = format!("{e:#}"), "PDF job failed");
                    store.update_live(&job_id, |j| {
                        j.view.progress = Progress::stage(Stage::Failed);
                        j.view.error = Some(format!("{e:#}"));
                        j.task = None;
                    })
                }
            }
        });

        inner.order.push_back(id.clone());
        inner.jobs.insert(
            id.clone(),
            Job {
                view: JobView {
                    id: id.clone(),
                    file_name,
                    model,
                    progress: Progress::stage(Stage::Queued),
                    error: None,
                    report: None,
                },
                annotated: None,
                task: Some(task.abort_handle()),
            },
        );
        id
    }

    pub fn view(&self, id: &str) -> Option<JobView> {
        self.inner.lock().unwrap().jobs.get(id).map(|j| j.view.clone())
    }

    /// Stops an unfinished job.
    pub fn cancel(&self, id: &str) -> Result<JobView, CancelError> {
        let mut inner = self.inner.lock().unwrap();
        let job = inner.jobs.get_mut(id).ok_or(CancelError::NotFound)?;
        let stage = job.view.progress.stage;
        if stage.is_finished() {
            return Err(CancelError::Finished(stage));
        }
        if let Some(task) = job.task.take() {
            task.abort();
        }
        job.view.progress = Progress::stage(Stage::Cancelled);
        tracing::info!(job = %id, file = %job.view.file_name, "job cancelled");
        Ok(job.view.clone())
    }

    /// File name and annotated PDF, once the job is done.
    pub fn annotated(&self, id: &str) -> Option<(String, Arc<Vec<u8>>)> {
        let inner = self.inner.lock().unwrap();
        let job = inner.jobs.get(id)?;
        Some((job.view.file_name.clone(), job.annotated.clone()?))
    }

    /// Applies `f` unless the job was cancelled meanwhile.
    fn update_live(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.inner.lock().unwrap().jobs.get_mut(id) {
            if job.view.progress.stage != Stage::Cancelled {
                f(job);
            }
        }
    }

    fn new_id(&self) -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        format!("{:x}{:x}", nanos & 0xffff_ffff_ffff, self.counter.fetch_add(1, Ordering::Relaxed))
    }
}

/// Drops the oldest finished jobs beyond the cap.
fn evict_finished(inner: &mut Inner) {
    while inner.order.len() >= MAX_JOBS {
        let Some(pos) = inner
            .order
            .iter()
            .position(|id| inner.jobs.get(id).is_some_and(|j| j.view.progress.stage.is_finished()))
        else {
            break;
        };
        let old = inner.order.remove(pos).expect("index valid");
        inner.jobs.remove(&old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scorer_core::Rubric;
    use scorer_llm::providers::MockProvider;

    #[tokio::test]
    async fn cancelled_job_stays_cancelled() {
        let store = Arc::new(JobStore::default());
        // Keep the runner busy so the job cannot finish before we cancel it.
        let hold = store.runner.acquire().await.unwrap();
        let scorer = Arc::new(Scorer::new(Arc::new(MockProvider::new(vec![])), Arc::new(Rubric::builtin_ai_journal())));
        let id = store.submit(scorer, b"%PDF-1.4".to_vec(), "wrong.pdf".into());

        assert_eq!(store.cancel(&id).ok().unwrap().progress.stage, Stage::Cancelled);
        assert!(matches!(store.cancel(&id), Err(CancelError::Finished(Stage::Cancelled))));
        assert!(matches!(store.cancel("nope"), Err(CancelError::NotFound)));

        drop(hold);
        tokio::task::yield_now().await;
        let view = store.view(&id).unwrap();
        assert_eq!(view.progress.stage, Stage::Cancelled);
        assert!(view.error.is_none() && store.annotated(&id).is_none());
    }
}
