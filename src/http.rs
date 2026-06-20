use crate::config::Config;
use crate::repos::RepoPaths;
use crate::state::{CancelOutcome, JobRegistry, JobRequest, JobState, JobStatus};
use crate::upload;
use axum::{
    extract::{Path as AxPath, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub secret: Arc<String>,
    pub registry: JobRegistry,
    pub queue_tx: mpsc::Sender<JobStatus>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/healthz", get(healthz))
        .route("/webhook/run", post(webhook_run))
        .route("/jobs", get(list_jobs))
        .route("/jobs/:id", get(get_job))
        .route("/jobs/:id/cancel", post(cancel_job))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

async fn dashboard() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebhookPayload {
    emulator: String,
    commit: String,
    #[serde(default)]
    force: bool,
}

fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn check_auth(headers: &HeaderMap, secret: &str) -> bool {
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(token.as_bytes(), secret.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn webhook_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if !check_auth(&headers, &state.secret) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing or invalid bearer token" })),
        )
            .into_response();
    }

    let payload: WebhookPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid payload: {e}") })),
            )
                .into_response()
        }
    };

    if state.config.emulator(&payload.emulator).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown emulator '{}'", payload.emulator) })),
        )
            .into_response();
    }
    if !is_full_sha(&payload.commit) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "commit must match ^[a-fA-F0-9]{40}$" })),
        )
            .into_response();
    }

    let req = JobRequest {
        emulator: payload.emulator,
        commit: payload.commit,
        force: payload.force,
    };

    let ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let job_id = format!("{ts}-{}-{}", req.emulator, &req.commit[..7]);

    let status = JobStatus::new(job_id.clone(), &req);
    let job_dir = state.registry.job_dir(&job_id);
    if let Err(e) = std::fs::create_dir_all(&job_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("creating job dir: {e}") })),
        )
            .into_response();
    }

    let _ = crate::state::write_json_atomic(&job_dir.join("request.json"), &req);
    if let Err(e) = state.registry.persist(&status) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("persisting status: {e}") })),
        )
            .into_response();
    }

    if !req.force {
        let archive_repo = RepoPaths::new(state.config.paths.repo_root.clone()).archive_repo();
        if archive_repo.join(".git").exists() {
            let archive_slug = &state.config.emulator(&req.emulator).unwrap().archive_slug;
            if let Ok(true) = upload::submission_exists(&archive_repo, archive_slug, &req.commit) {
                let mut s = status.clone();
                s.state = JobState::AlreadyCompleted;
                s.finished_at = Some(chrono::Utc::now());
                s.message = Some("submission already exists for emulator+commit".to_string());
                let _ = state.registry.persist(&s);
                return (
                    StatusCode::OK,
                    Json(json!({
                        "job_id": job_id,
                        "state": "already_completed",
                        "message": "submission already exists for emulator+commit"
                    })),
                )
                    .into_response();
            }
        }
    }

    if let Err(e) = state.queue_tx.send(status).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("enqueue failed: {e}") })),
        )
            .into_response();
    }

    (
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id, "state": "queued" })),
    )
        .into_response()
}

async fn get_job(State(state): State<AppState>, AxPath(id): AxPath<String>) -> impl IntoResponse {
    match state.registry.get(&id) {
        Some(status) => {
            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "job not found" })),
        )
            .into_response(),
    }
}

async fn cancel_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    if !check_auth(&headers, &state.secret) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing or invalid bearer token" })),
        )
            .into_response();
    }

    match state.registry.request_cancel(&id) {
        Ok(CancelOutcome::Cancelled) => (
            StatusCode::OK,
            Json(json!({ "job_id": id, "state": "cancelled" })),
        )
            .into_response(),
        Ok(CancelOutcome::NotCancellable(current)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "job is not queued and cannot be cancelled",
                "state": current,
            })),
        )
            .into_response(),
        Ok(CancelOutcome::NotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "job not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("cancel failed: {e}") })),
        )
            .into_response(),
    }
}

async fn list_jobs(State(state): State<AppState>) -> impl IntoResponse {
    let jobs = state.registry.list();
    Json(serde_json::to_value(jobs).unwrap())
}
