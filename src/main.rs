mod archive;
mod config;
mod docker;
mod error;
mod http;
mod job;
mod postprocess;
mod proc;
mod repos;
mod roms;
mod screenshotter;
mod state;
mod upload;
mod validate;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use state::JobRegistry;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(name = "emu-shot-orchestrator", version, about)]
struct Cli {
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = Arc::new(Config::load(&cli.config).context("loading config")?);
    let secret = Arc::new(
        config
            .webhook_secret()
            .context("resolving webhook secret")?,
    );

    std::fs::create_dir_all(&config.paths.job_root).ok();
    std::fs::create_dir_all(&config.paths.repo_root).ok();

    let registry = JobRegistry::new(config.paths.job_root.clone());

    {
        let registry = registry.clone();
        let retention_hours = config.server.job_retention_hours;
        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(30 * 60);
            loop {
                cleanup_old_jobs(&registry, retention_hours);
                tokio::time::sleep(interval).await;
            }
        });
    }

    let (tx, mut rx) = mpsc::channel::<state::JobStatus>(64);

    {
        let config = config.clone();
        let registry = registry.clone();
        tokio::spawn(async move {
            while let Some(status) = rx.recv().await {
                // The channel holds a snapshot taken at enqueue time; re-check the
                // registry (the source of truth) so jobs cancelled while queued are
                // skipped instead of run.
                if let Some(current) = registry.get(&status.id) {
                    if current.state == state::JobState::Cancelled {
                        tracing::info!(job = %status.id, "skipping cancelled job");
                        continue;
                    }
                }
                let config = config.clone();
                let registry = registry.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    job::run_pipeline(&config, &registry, status);
                })
                .await;
            }
        });
    }

    let app_state = http::AppState {
        config: config.clone(),
        secret,
        registry,
        queue_tx: tx,
    };

    let addr: std::net::SocketAddr = config
        .server
        .bind
        .parse()
        .with_context(|| format!("parsing bind address '{}'", config.server.bind))?;

    tracing::info!(%addr, "emu-shot-orchestrator listening");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    axum::serve(listener, http::router(app_state))
        .await
        .context("axum serve")?;

    Ok(())
}

fn cleanup_old_jobs(registry: &JobRegistry, retention_hours: u64) {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(retention_hours as i64);
    for status in registry.list() {
        let terminal = matches!(
            status.state,
            state::JobState::Completed
                | state::JobState::Failed
                | state::JobState::AlreadyCompleted
                | state::JobState::Cancelled
        );
        if !terminal {
            continue;
        }
        let finished = status.finished_at.unwrap_or(status.created_at);
        if finished < cutoff {
            if let Err(e) = registry.remove(&status.id) {
                tracing::warn!(job = %status.id, error = %e, "failed to remove old job");
            } else {
                tracing::info!(job = %status.id, "removed old job (retention expired)");
            }
        }
    }
}
