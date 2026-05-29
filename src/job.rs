use crate::config::Config;
use crate::error::{JobError, JobResult};
use crate::state::{JobRegistry, JobRequest, JobStage, JobState, JobStatus};
use crate::{docker, postprocess, repos, roms, screenshotter, upload, validate};
use chrono::{DateTime, Utc};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

fn log_append(path: &Path, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

pub fn run_pipeline(config: &Config, registry: &JobRegistry, mut status: JobStatus) {
    let id = status.id.clone();
    status.state = JobState::Running;
    status.started_at = Some(Utc::now());
    let _ = registry.persist(&status);

    let req = JobRequest {
        emulator: status.emulator.clone(),
        commit: status.commit.clone(),
        force: status.force,
    };

    match run_stages(config, registry, &mut status, &req) {
        Ok(Outcome::Completed) => {
            status.state = JobState::Completed;
            status.stage = JobStage::Done;
            status.finished_at = Some(Utc::now());
            let _ = registry.persist(&status);
            tracing::info!(job = %id, "job completed");
        }
        Ok(Outcome::AlreadyCompleted) => {
            status.state = JobState::AlreadyCompleted;
            status.finished_at = Some(Utc::now());
            status.message = Some("submission already exists for emulator+commit".to_string());
            let _ = registry.persist(&status);
            tracing::info!(job = %id, "job already completed (idempotent skip)");
        }
        Err(e) => {
            status.state = JobState::Failed;
            status.finished_at = Some(Utc::now());
            status.failure_kind = Some(e.kind().to_string());
            status.error = Some(e.to_string());
            let _ = registry.persist(&status);
            tracing::error!(job = %id, stage = ?status.stage, kind = e.kind(), error = %e, "job failed");
        }
    }
}

enum Outcome {
    Completed,
    AlreadyCompleted,
}

fn run_stages(
    config: &Config,
    registry: &JobRegistry,
    status: &mut JobStatus,
    req: &JobRequest,
) -> JobResult<Outcome> {
    let emu = config
        .emulator(&req.emulator)
        .ok_or_else(|| JobError::Validation(format!("unknown emulator '{}'", req.emulator)))?
        .clone();

    let job_dir = registry.job_dir(&status.id);
    let logs = job_dir.join("logs");
    std::fs::create_dir_all(&logs)?;
    let clone_log = logs.join("clone.log");
    let build_log = logs.join("build.log");
    let roms_log = logs.join("roms.log");
    let run_log = logs.join("run.log");
    let upload_log = logs.join("upload.log");

    let repo_paths = repos::RepoPaths::new(config.paths.repo_root.clone());

    set_stage(registry, status, JobStage::PrepareRepos);
    let archive_repo = repos::prepare_archive_repo(
        "https://github.com/ioncodes/emu.layle.dev.git",
        &repo_paths,
        &clone_log,
    )?;

    if !req.force && upload::submission_exists(&archive_repo, &emu.archive_slug, &req.commit)? {
        return Ok(Outcome::AlreadyCompleted);
    }

    let screenshotter_dir = emu.dir.clone();
    let emulator_worktree = repos::prepare_emulator_repo(
        &emu.emulator_repo,
        &emu.slug,
        &req.commit,
        &repo_paths,
        &clone_log,
        &emu.skip_submodules,
        emu.shallow_submodules,
    )?;
    let emu_short = repos::short_sha(&req.commit);
    let shotter_short = docker::dir_revision(&screenshotter_dir)?;

    set_stage(registry, status, JobStage::ReadGameFiles);
    let game_set = screenshotter::load_and_stage_game_set(&screenshotter_dir, &job_dir)?;
    let output_mode = emu.output_mode;
    let ids: Vec<String> = game_set.titlemap.iter().map(|t| t.id.clone()).collect();

    set_stage(registry, status, JobStage::ResolveRoms);
    let rom_base = emu.rom_base(&config.paths.rom_root);
    let resolved = roms::resolve_games(&game_set, &rom_base)?;
    log_append(
        &roms_log,
        &format!(
            "resolved {} games under {}",
            resolved.len(),
            rom_base.display()
        ),
    );

    set_stage(registry, status, JobStage::BuildImage);
    let tag = docker::image_tag(&emu.slug, &emu_short, &shotter_short);
    docker::build_image(
        &emu.slug,
        &screenshotter_dir,
        &emulator_worktree,
        &tag,
        &build_log,
    )?;

    let output_root = job_dir.join("output");
    std::fs::create_dir_all(&output_root)?;

    let total = resolved.len();
    status.games_total = Some(total as u32);
    status.games_done = Some(0);
    status.eta_at = None;
    let _ = registry.persist(status);
    let games_started_at = Utc::now();

    set_stage(registry, status, JobStage::RunScreenshotter);
    let skipped_ids = process_games_concurrently(
        &emu,
        &resolved,
        &status.id.clone(),
        &job_dir,
        &roms_log,
        &run_log,
        &config.paths.secret_root,
        &tag,
        &output_root,
        output_mode,
        &config.postprocess,
        (emu.max_parallel_stages as usize).max(1),
        (emu.max_parallel_games as usize).max(1),
        registry,
        status,
        games_started_at,
        total,
    )?;

     let rendered_ids: Vec<String> = ids
        .iter()
        .filter(|id| !skipped_ids.contains(id))
        .cloned()
        .collect();
    if rendered_ids.is_empty() {
        return Err(JobError::PostProcess(format!(
            "no usable frames from any of the {} game(s)",
            ids.len()
        )));
    }
    if !skipped_ids.is_empty() {
        status.games_skipped = Some(skipped_ids.len() as u32);
        status.message = Some(format!(
            "skipped {} game(s) with no usable frames: {}",
            skipped_ids.len(),
            skipped_ids.join(", ")
        ));
        let _ = registry.persist(status);
    }

    set_stage(registry, status, JobStage::ValidateOutput);
    validate::validate_output(&output_root, &rendered_ids, output_mode)?;

    set_stage(registry, status, JobStage::Upload);
    upload::run_uploader(
        &archive_repo,
        &emu.archive_slug,
        &emulator_worktree,
        &output_root,
        &job_dir.join("titlemap.txt"),
        &config.upload.r2_env_file,
        &config.upload.submitted_by,
        config.upload.workers,
        config.upload.no_push,
        config.upload.dry_run,
        &upload_log,
    )?;

    Ok(Outcome::Completed)
}

#[allow(clippy::too_many_arguments)]
fn process_games_concurrently(
    emu: &crate::config::EmulatorConfig,
    resolved: &[roms::ResolvedGame],
    job_id: &str,
    job_dir: &Path,
    roms_log: &Path,
    run_log: &Path,
    secret_root: &Path,
    tag: &str,
    output_root: &Path,
    output_mode: crate::config::OutputMode,
    ppcfg: &crate::config::PostProcessConfig,
    stage_pool: usize,
    run_pool: usize,
    registry: &JobRegistry,
    status: &mut JobStatus,
    started_at: DateTime<Utc>,
    total: usize,
) -> JobResult<Vec<String>> {
    // pending -> stage workers -> ready -> run workers -> prog channel
    let (pending_tx, pending_rx) = mpsc::channel::<roms::ResolvedGame>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<(roms::ResolvedGame, String)>(run_pool);
    let (prog_tx, prog_rx) = mpsc::channel::<JobResult<(String, bool)>>();

    for g in resolved {
        pending_tx.send(g.clone()).expect("pending queue closed");
    }
    drop(pending_tx);

    let pending_rx = Arc::new(Mutex::new(pending_rx));
    let ready_rx = Arc::new(Mutex::new(ready_rx));
    let cancel = Arc::new(AtomicBool::new(false));

    let mut stage_handles = Vec::with_capacity(stage_pool);
    for _ in 0..stage_pool {
        let rx = pending_rx.clone();
        let tx = ready_tx.clone();
        let cancel = cancel.clone();
        let ptx = prog_tx.clone();
        let emu = emu.clone();
        let roms_log = roms_log.to_path_buf();

        let job_id_s = job_id.to_string();
        stage_handles.push(thread::spawn(move || loop {
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            let game = match rx.lock().unwrap().recv() {
                Ok(g) => g,
                Err(_) => return,
            };
            let mut sink = |l: &str| log_append(&roms_log, l);
            match roms::stage_one_game(&emu, &game, &job_id_s, Path::new("/staging"), &mut sink) {
                Ok(ci) => {
                    if tx.send((game, ci)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = ptx.send(Err(e));
                    cancel.store(true, Ordering::SeqCst);
                    return;
                }
            }
        }));
    }
    drop(ready_tx);

    let mut run_handles = Vec::with_capacity(run_pool);
    for _ in 0..run_pool {
        let rx = ready_rx.clone();
        let cancel = cancel.clone();
        let ptx = prog_tx.clone();
        let emu = emu.clone();
        let job_id = job_id.to_string();
        let job_dir = job_dir.to_path_buf();
        let roms_log = roms_log.to_path_buf();
        let run_log = run_log.to_path_buf();
        let secret_root = secret_root.to_path_buf();
        let tag = tag.to_string();
        let output_root = output_root.to_path_buf();
        let ppcfg = ppcfg.clone();

        run_handles.push(thread::spawn(move || loop {
            // Always drain the bounded `ready` channel, even after cancel, so
            // stage workers blocked inside `ready_tx.send()` can unblock and
            // return — otherwise the join() below deadlocks. Once cancelled we
            // just discard the staged game instead of running it.
            let (game, container_input) = match rx.lock().unwrap().recv() {
                Ok(item) => item,
                Err(_) => return,
            };
            if cancel.load(Ordering::SeqCst) {
                continue;
            }
            let result = run_and_postprocess(
                &emu,
                &job_id,
                &game,
                &container_input,
                &job_dir,
                &roms_log,
                &run_log,
                &secret_root,
                &tag,
                &output_root,
                output_mode,
                &ppcfg,
            );
            let _ = ptx.send(result);
        }));
    }
    drop(prog_tx);

    let mut done = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut first_err: Option<JobError> = None;
    while let Ok(r) = prog_rx.recv() {
        match r {
            Ok((_id, true)) => {
                done += 1;
                update_progress(registry, status, started_at, done, total);
            }
            Ok((id, false)) => {
                // recorded once by the caller (run_stages) after the batch
                skipped.push(id);
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    for h in stage_handles {
        let _ = h.join();
    }
    for h in run_handles {
        let _ = h.join();
    }

    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(skipped)
}

#[allow(clippy::too_many_arguments)]
fn run_and_postprocess(
    emu: &crate::config::EmulatorConfig,
    job_id: &str,
    game: &roms::ResolvedGame,
    container_input: &str,
    job_dir: &Path,
    roms_log: &Path,
    run_log: &Path,
    secret_root: &Path,
    tag: &str,
    output_root: &Path,
    output_mode: crate::config::OutputMode,
    ppcfg: &crate::config::PostProcessConfig,
) -> JobResult<(String, bool)> {
    docker::run_screenshotter_one(
        emu,
        job_id,
        &game.id,
        container_input,
        job_dir,
        secret_root,
        tag,
        run_log,
    )?;

    let mut sink = |l: &str| log_append(run_log, l);
    let produced = postprocess::postprocess_output(
        output_root,
        std::slice::from_ref(&game.id),
        output_mode,
        ppcfg,
        &mut sink,
    )?
    .is_empty();

    let _ = std::fs::remove_dir_all(Path::new("/staging").join(job_id).join(&game.id));
    log_append(roms_log, &format!("freed staged inputs for {}", game.id));

    Ok((game.id.clone(), produced))
}

fn update_progress(
    registry: &JobRegistry,
    status: &mut JobStatus,
    started_at: DateTime<Utc>,
    done: usize,
    total: usize,
) {
    status.games_done = Some(done as u32);
    let now = Utc::now();
    let elapsed = (now - started_at).num_milliseconds().max(1) as f64 / 1000.0;
    let remaining = total.saturating_sub(done);
    if done > 0 && remaining > 0 {
        let per_game = elapsed / done as f64;
        let eta_secs = (per_game * remaining as f64).round() as i64;
        status.eta_at = Some(now + chrono::Duration::seconds(eta_secs));
    } else if remaining == 0 {
        status.eta_at = Some(now);
    }
    let _ = registry.persist(status);
}

fn set_stage(registry: &JobRegistry, status: &mut JobStatus, stage: JobStage) {
    status.stage = stage;
    let _ = registry.persist(status);
    tracing::info!(job = %status.id, stage = ?stage, "stage");
}
