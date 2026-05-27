use crate::config::Config;
use crate::error::{JobError, JobResult};
use crate::state::{JobRegistry, JobRequest, JobStage, JobState, JobStatus};
use crate::{docker, postprocess, repos, roms, screenshotter, upload, validate};
use chrono::Utc;
use std::io::Write;
use std::path::Path;

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

    let batch_size = (emu.max_parallel_games as usize).max(1);
    let nbatches = resolved.len().div_ceil(batch_size);
    for (bi, batch) in resolved.chunks(batch_size).enumerate() {
        let batch_ids: Vec<String> = batch.iter().map(|g| g.id.clone()).collect();
        log_append(
            &roms_log,
            &format!(
                "=== batch {}/{}: {} game(s) ===",
                bi + 1,
                nbatches,
                batch.len()
            ),
        );

        set_stage(registry, status, JobStage::StageInputs);
        {
            let mut sink = |l: &str| log_append(&roms_log, l);
            roms::stage_games(&emu, &req.commit, batch, &job_dir, &mut sink)?;
        }

        set_stage(registry, status, JobStage::RunScreenshotter);
        docker::run_screenshotter(
            &emu,
            &status.id,
            &job_dir,
            &config.paths.secret_root,
            &tag,
            &run_log,
        )?;

        set_stage(registry, status, JobStage::PostprocessFrames);
        {
            let mut sink = |l: &str| log_append(&run_log, l);
            postprocess::postprocess_output(
                &output_root,
                &batch_ids,
                output_mode,
                &config.postprocess,
                &mut sink,
            )?;
        }

        cleanup_batch_inputs(&job_dir, &batch_ids, &roms_log);
    }

    set_stage(registry, status, JobStage::ValidateOutput);
    validate::validate_output(&output_root, &ids, output_mode)?;

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

fn cleanup_batch_inputs(job_dir: &Path, ids: &[String], log: &Path) {
    let input = job_dir.join("input");
    let scratch = job_dir.join("scratch");
    for id in ids {
        let _ = std::fs::remove_dir_all(input.join(id));
        let _ = std::fs::remove_dir_all(scratch.join(id));
    }
    log_append(
        log,
        &format!("freed staged inputs for {} game(s)", ids.len()),
    );
}

fn set_stage(registry: &JobRegistry, status: &mut JobStatus, stage: JobStage) {
    status.stage = stage;
    let _ = registry.persist(status);
    tracing::info!(job = %status.id, stage = ?stage, "stage");
}
