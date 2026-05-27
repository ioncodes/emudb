use crate::error::{JobError, JobResult};
use crate::proc::run_checked;
use std::path::Path;

pub fn submission_exists(archive_repo: &Path, archive_slug: &str, commit: &str) -> JobResult<bool> {
    let dir = archive_repo
        .join("meta")
        .join("submissions")
        .join(archive_slug);
    if !dir.is_dir() {
        return Ok(false);
    }
    let short = commit.chars().take(7).collect::<String>();
    let suffix = format!("-{short}.json");

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(&suffix) {
            continue;
        }
        let text = match std::fs::read_to_string(entry.path()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let parsed: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(c) = parsed.get("commit").and_then(|v| v.as_str()) {
            if c.eq_ignore_ascii_case(commit) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
pub fn run_uploader(
    archive_repo: &Path,
    archive_slug: &str,
    emulator_worktree: &Path,
    job_output: &Path,
    job_titlemap: &Path,
    r2_env_file: &Path,
    submitted_by: &str,
    workers: u32,
    no_push: bool,
    dry_run: bool,
    log: &Path,
) -> JobResult<()> {
    let script = archive_repo.join("tools").join("submit-screenshots.py");
    if !script.is_file() {
        return Err(JobError::Upload(format!(
            "uploader script not found: {}",
            script.display()
        )));
    }

    let mut args = vec![
        "run".into(),
        script.to_string_lossy().to_string(),
        "--emulator".into(),
        archive_slug.to_string(),
        "--emu-repo".into(),
        emulator_worktree.to_string_lossy().to_string(),
        "--input".into(),
        job_output.to_string_lossy().to_string(),
        "--archive-repo".into(),
        archive_repo.to_string_lossy().to_string(),
        "--env-file".into(),
        r2_env_file.to_string_lossy().to_string(),
        "--title-map".into(),
        job_titlemap.to_string_lossy().to_string(),
        "--submitted-by".into(),
        submitted_by.to_string(),
        "--workers".into(),
        workers.to_string(),
    ];
    if no_push {
        args.push("--no-push".into());
    }
    if dry_run {
        args.push("--dry-run".into());
    }

    run_checked("uv", &args, None, log, &[], JobError::Upload)?;
    Ok(())
}
