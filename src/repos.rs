use crate::error::{JobError, JobResult};
use crate::proc::{run_checked, run_logged};
use std::path::{Path, PathBuf};

pub struct RepoPaths {
    pub repo_root: PathBuf,
}

impl RepoPaths {
    pub fn new(repo_root: PathBuf) -> Self {
        RepoPaths { repo_root }
    }

    pub fn archive_repo(&self) -> PathBuf {
        self.repo_root.join("emu.layle.dev")
    }

    pub fn emulator_worktree(&self, slug: &str) -> PathBuf {
        self.repo_root.join("emulators").join(slug).join("worktree")
    }
}

fn repo_err(s: String) -> JobError {
    JobError::Repo(s)
}

pub fn ensure_repo(url: &str, path: &Path, log: &Path) -> JobResult<()> {
    if path.join(".git").exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    run_checked(
        "git",
        &[
            "clone".into(),
            url.to_string(),
            path.to_string_lossy().to_string(),
        ],
        None,
        log,
        &[],
        repo_err,
    )?;
    Ok(())
}

pub fn fetch_repo(path: &Path, log: &Path) -> JobResult<()> {
    run_checked(
        "git",
        &[
            "-C".into(),
            path.to_string_lossy().to_string(),
            "fetch".into(),
            "--all".into(),
            "--tags".into(),
            "--prune".into(),
        ],
        None,
        log,
        &[],
        repo_err,
    )?;
    Ok(())
}

pub fn checkout(path: &Path, refname: &str, log: &Path) -> JobResult<()> {
    run_checked(
        "git",
        &[
            "-C".into(),
            path.to_string_lossy().to_string(),
            "checkout".into(),
            refname.to_string(),
        ],
        None,
        log,
        &[],
        repo_err,
    )?;
    Ok(())
}

pub fn reset_hard(path: &Path, remote_ref: &str, log: &Path) -> JobResult<()> {
    run_checked(
        "git",
        &[
            "-C".into(),
            path.to_string_lossy().to_string(),
            "reset".into(),
            "--hard".into(),
            remote_ref.to_string(),
        ],
        None,
        log,
        &[],
        repo_err,
    )?;
    Ok(())
}

pub fn submodule_update(path: &Path, log: &Path) -> JobResult<()> {
    let p = path.to_string_lossy().to_string();
    let git = |args: &[&str]| {
        let mut v = vec!["-C".to_string(), p.clone()];
        v.extend(args.iter().map(|s| s.to_string()));
        v
    };

    let _ = run_logged(
        "git",
        &git(&["submodule", "sync", "--recursive"]),
        None,
        log,
        &[],
    );
    let first = run_logged(
        "git",
        &git(&[
            "submodule",
            "update",
            "--init",
            "--recursive",
            "--force",
            "--jobs",
            "4",
        ]),
        None,
        log,
        &[],
    )?;
    if first.status == 0 {
        return Ok(());
    }

    tracing::warn!(repo = %p, "submodule update failed; deinit + retry");
    let _ = run_logged(
        "git",
        &git(&["submodule", "deinit", "-f", "--all"]),
        None,
        log,
        &[],
    );
    run_checked(
        "git",
        &git(&[
            "submodule",
            "update",
            "--init",
            "--recursive",
            "--force",
            "--jobs",
            "4",
        ]),
        None,
        log,
        &[],
        repo_err,
    )?;
    Ok(())
}

pub fn rev_parse_head(path: &Path, log: &Path) -> JobResult<String> {
    let out = run_checked(
        "git",
        &[
            "-C".into(),
            path.to_string_lossy().to_string(),
            "rev-parse".into(),
            "HEAD".into(),
        ],
        None,
        log,
        &[],
        repo_err,
    )?;
    Ok(out.stdout.trim().to_string())
}

pub fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

pub fn prepare_archive_repo(url: &str, paths: &RepoPaths, log: &Path) -> JobResult<PathBuf> {
    let path = paths.archive_repo();
    ensure_repo(url, &path, log)?;
    fetch_repo(&path, log)?;
    checkout(&path, "main", log)?;
    reset_hard(&path, "origin/main", log)?;
    Ok(path)
}

pub fn prepare_emulator_repo(
    url: &str,
    slug: &str,
    commit: &str,
    paths: &RepoPaths,
    log: &Path,
) -> JobResult<PathBuf> {
    let path = paths.emulator_worktree(slug);
    ensure_repo(url, &path, log)?;
    fetch_repo(&path, log)?;
    checkout(&path, commit, log)?;
    submodule_update(&path, log)?;

    let head = rev_parse_head(&path, log)?;
    if !head.eq_ignore_ascii_case(commit) {
        return Err(JobError::Repo(format!(
            "emulator HEAD {head} does not match requested commit {commit}"
        )));
    }
    Ok(path)
}
