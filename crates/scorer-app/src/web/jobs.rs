//! In-memory PDF jobs: upload -> background scoring -> poll -> download.
//! Jobs run one at a time (the LLM server is the bottleneck); the store
//! keeps the most recent `MAX_JOBS` and forgets them on restart.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use scorer_engine::{Progress, ScoreReport, Scorer, Stage};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::pdf;

const MAX_JOBS: usize = 30;

#[derive(Clone, Serialize)]
pub struct JobView {
    pub id: String,
    pub file_name: String,
    pub progress: Progress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Arc<ScoreReport>>,
}

struct Job {
    view: JobView,
    annotated: Option<Arc<Vec<u8>>>,
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
        self.insert(Job {
            view: JobView {
                id: id.clone(),
                file_name: file_name.clone(),
                progress: Progress::stage(Stage::Queued),
                error: None,
                report: None,
            },
            annotated: None,
        });

        let store = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            let _permit = store.runner.acquire().await.expect("semaphore never closed");
            let on_progress = |p: Progress| store.update(&job_id, |j| j.view.progress = p);
            match pdf::score_pdf(&scorer, Arc::new(pdf), &file_name, &on_progress).await {
                Ok(outcome) => store.update(&job_id, |j| {
                    j.view.progress = Progress::stage(Stage::Done);
                    j.view.report = Some(Arc::new(outcome.report));
                    j.annotated = Some(Arc::new(outcome.annotated));
                }),
                Err(e) => {
                    tracing::error!(job = %job_id, error = format!("{e:#}"), "PDF job failed");
                    store.update(&job_id, |j| {
                        j.view.progress = Progress::stage(Stage::Failed);
                        j.view.error = Some(format!("{e:#}"));
                    })
                }
            }
        });
        id
    }

    pub fn view(&self, id: &str) -> Option<JobView> {
        self.inner.lock().unwrap().jobs.get(id).map(|j| j.view.clone())
    }

    /// File name and annotated PDF, once the job is done.
    pub fn annotated(&self, id: &str) -> Option<(String, Arc<Vec<u8>>)> {
        let inner = self.inner.lock().unwrap();
        let job = inner.jobs.get(id)?;
        Some((job.view.file_name.clone(), job.annotated.clone()?))
    }

    fn insert(&self, job: Job) {
        let mut inner = self.inner.lock().unwrap();
        // Evict the oldest finished jobs beyond the cap.
        while inner.order.len() >= MAX_JOBS {
            let Some(pos) = inner.order.iter().position(|id| {
                inner.jobs.get(id).is_some_and(|j| matches!(j.view.progress.stage, Stage::Done | Stage::Failed))
            }) else {
                break;
            };
            let old = inner.order.remove(pos).expect("index valid");
            inner.jobs.remove(&old);
        }
        inner.order.push_back(job.view.id.clone());
        inner.jobs.insert(job.view.id.clone(), job);
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.inner.lock().unwrap().jobs.get_mut(id) {
            f(job);
        }
    }

    fn new_id(&self) -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        format!("{:x}{:x}", nanos & 0xffff_ffff_ffff, self.counter.fetch_add(1, Ordering::Relaxed))
    }
}
