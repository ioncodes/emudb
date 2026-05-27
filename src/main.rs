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

    let (tx, mut rx) = mpsc::channel::<state::JobStatus>(64);

    {
        let config = config.clone();
        let registry = registry.clone();
        tokio::spawn(async move {
            while let Some(status) = rx.recv().await {
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
