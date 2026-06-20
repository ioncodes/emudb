use crate::error::JobResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed,
    AlreadyCompleted,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    Received,
    PrepareRepos,
    ReadGameFiles,
    ResolveRoms,
    StageInputs,
    BuildImage,
    RunScreenshotter,
    PostprocessFrames,
    ValidateOutput,
    Upload,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub emulator: String,
    pub commit: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatus {
    pub id: String,
    pub emulator: String,
    pub commit: String,
    pub force: bool,
    pub state: JobState,
    pub stage: JobStage,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,

    pub failure_kind: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub games_total: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub games_done: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub games_skipped: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_at: Option<DateTime<Utc>>,
}

impl JobStatus {
    pub fn new(id: String, req: &JobRequest) -> Self {
        JobStatus {
            id,
            emulator: req.emulator.clone(),
            commit: req.commit.clone(),
            force: req.force,
            state: JobState::Queued,
            stage: JobStage::Received,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            error: None,
            failure_kind: None,
            message: None,
            games_total: None,
            games_done: None,
            games_skipped: None,
            eta_at: None,
        }
    }
}

pub enum CancelOutcome {
    Cancelled,
    /// Job exists but is past the queue (running or terminal); carries its state.
    NotCancellable(JobState),
    NotFound,
}

#[derive(Clone)]
pub struct JobRegistry {
    job_root: PathBuf,
    inner: Arc<Mutex<HashMap<String, JobStatus>>>,
}

impl JobRegistry {
    pub fn new(job_root: PathBuf) -> Self {
        JobRegistry {
            job_root,
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn job_dir(&self, id: &str) -> PathBuf {
        self.job_root.join(id)
    }

    fn status_path(&self, id: &str) -> PathBuf {
        self.job_dir(id).join("status.json")
    }

    pub fn persist(&self, status: &JobStatus) -> JobResult<()> {
        let path = self.status_path(&status.id);
        write_json_atomic(&path, status)?;
        self.inner
            .lock()
            .unwrap()
            .insert(status.id.clone(), status.clone());
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<JobStatus> {
        if let Some(s) = self.inner.lock().unwrap().get(id) {
            return Some(s.clone());
        }
        let path = self.status_path(id);
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Attempt to cancel a job. Only jobs still in the `Queued` state can be
    /// cancelled; once running or terminal, the request is rejected.
    pub fn request_cancel(&self, id: &str) -> JobResult<CancelOutcome> {
        let Some(mut status) = self.get(id) else {
            return Ok(CancelOutcome::NotFound);
        };
        if status.state != JobState::Queued {
            return Ok(CancelOutcome::NotCancellable(status.state));
        }
        status.state = JobState::Cancelled;
        status.finished_at = Some(Utc::now());
        status.message = Some("cancelled before execution".to_string());
        self.persist(&status)?;
        Ok(CancelOutcome::Cancelled)
    }

    pub fn remove(&self, id: &str) -> JobResult<()> {
        self.inner.lock().unwrap().remove(id);
        let path = self.job_dir(id);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<JobStatus> {
        let mut map: HashMap<String, JobStatus> = self.inner.lock().unwrap().clone();
        if let Ok(entries) = std::fs::read_dir(&self.job_root) {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().to_string();
                if map.contains_key(&id) {
                    continue;
                }
                let path = entry.path().join("status.json");
                if let Ok(text) = std::fs::read_to_string(path) {
                    if let Ok(status) = serde_json::from_str::<JobStatus>(&text) {
                        map.insert(id, status);
                    }
                }
            }
        }
        let mut v: Vec<JobStatus> = map.into_values().collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        v
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> JobResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value)
        .map_err(|e| crate::error::JobError::Other(format!("serializing json: {e}")))?;
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
