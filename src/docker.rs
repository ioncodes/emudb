use crate::config::{EmulatorConfig, GpuMode};
use crate::error::{JobError, JobResult};
use crate::proc::{run_checked, run_checked_with_timeout};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn image_tag(slug: &str, emu_short: &str, shotter_short: &str) -> String {
    format!("emu-shot/{slug}:{emu_short}-shotter-{shotter_short}")
}

pub fn dir_revision(dir: &Path) -> JobResult<String> {
    let mut files: Vec<_> = walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();

    let mut hasher = Sha256::new();
    for f in &files {
        let rel = f.strip_prefix(dir).unwrap_or(f);
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update([0]);
        let bytes = std::fs::read(f)
            .map_err(|e| JobError::DockerBuild(format!("hashing {}: {e}", f.display())))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let digest = hasher.finalize();
    Ok(hex7(&digest))
}

fn hex7(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(7);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
        if s.len() >= 7 {
            break;
        }
    }
    s.truncate(7);
    s
}

pub fn build_image(
    slug: &str,
    screenshotter_dir: &Path,
    emulator_worktree: &Path,
    tag: &str,
    log: &Path,
) -> JobResult<()> {
    let dockerfile = screenshotter_dir.join("Dockerfile");
    let args = vec![
        "buildx".into(),
        "build".into(),
        "--load".into(),
        "--pull".into(),
        "--file".into(),
        dockerfile.to_string_lossy().to_string(),
        "--tag".into(),
        tag.to_string(),
        "--build-context".into(),
        format!("emulator={}", emulator_worktree.to_string_lossy()),
        screenshotter_dir.to_string_lossy().to_string(),
    ];
    let _ = slug;
    run_checked("docker", &args, None, log, &[], JobError::DockerBuild)?;
    Ok(())
}

pub fn run_screenshotter_one(
    emu: &EmulatorConfig,
    job_id: &str,
    game_id: &str,
    container_input: &str,
    job_dir: &Path,
    secret_root: &Path,
    tag: &str,
    log: &Path,
) -> JobResult<()> {
    let output = job_dir.join("output");
    let secrets = secret_root.join(&emu.slug);

    std::fs::create_dir_all(output.join(game_id))?;

    let container_name = format!("shot-{}-{}-{}", emu.slug, job_id, game_id);
    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        container_name.clone(),
    ];
    match emu.gpu_mode {
        GpuMode::Nvidia => {
            args.push("--gpus".into());
            args.push("all".into());
        }
        GpuMode::Dri => {
            args.push("--device".into());
            args.push("/dev/dri".into());
        }
        GpuMode::None => {}
    }
    args.push("--network".into());
    args.push("none".into());

    args.push("-v".into());
    args.push("emudb-staging:/staging:ro".into());
    args.push("-v".into());
    args.push(format!("{}:/output", output.to_string_lossy()));

    if secrets.exists() {
        args.push("-v".into());
        args.push(format!("{}:/config:ro", secrets.to_string_lossy()));
    }

    for m in &emu.docker_mounts {
        args.push("-v".into());
        args.push(m.clone());
    }
    for (k, v) in &emu.docker_env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }

    args.push(tag.to_string());

    let container_output = format!("/output/{game_id}");
    for a in &emu.per_game_args {
        args.push(
            a.replace("{disc}", container_input)
                .replace("{out}", &container_output),
        );
    }

    match emu.timeout_seconds {
        Some(secs) => {
            run_checked_with_timeout(
                "docker",
                &args,
                None,
                log,
                &[],
                JobError::DockerRun,
                std::time::Duration::from_secs(secs),
                &container_name,
            )?;
        }
        None => {
            run_checked("docker", &args, None, log, &[], JobError::DockerRun)?;
        }
    }
    Ok(())
}
