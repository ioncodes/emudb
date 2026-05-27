use crate::archive;
use crate::config::{EmulatorConfig, ExtKind};
use crate::error::{JobError, JobResult};
use crate::screenshotter::{entry_stem, GameSet};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedGame {
    pub id: String,
    pub title: String,

    #[allow(dead_code)]
    pub rel_entry: String,

    pub rom_path: PathBuf,
    pub ext: String,
}

#[derive(Debug, Serialize)]
pub struct ManifestGame {
    pub id: String,
    pub title: String,

    pub input: String,

    pub output_dir: String,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub emulator: String,
    pub commit: String,
    pub games: Vec<ManifestGame>,
}

pub fn resolve_under_root(rom_root: &Path, rel_entry: &str) -> JobResult<PathBuf> {
    let normalized = rel_entry.replace('\\', "/");
    let p = Path::new(&normalized);
    if p.is_absolute() {
        return Err(JobError::RomResolution(format!(
            "gamelist entry is an absolute path: {rel_entry:?}"
        )));
    }
    let mut out = rom_root.to_path_buf();
    for comp in p.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(JobError::RomResolution(format!(
                    "gamelist entry contains illegal path component: {rel_entry:?}"
                )))
            }
        }
    }
    if !out.starts_with(rom_root) {
        return Err(JobError::RomResolution(format!(
            "resolved path escapes rom root: {rel_entry:?}"
        )));
    }
    Ok(out)
}

pub fn resolve_games(set: &GameSet, rom_root: &Path) -> JobResult<Vec<ResolvedGame>> {
    let mut stem_to_entry: HashMap<String, String> = HashMap::new();
    for entry in &set.gamelist {
        if let Some(stem) = entry_stem(entry) {
            stem_to_entry.insert(stem, entry.clone());
        }
    }

    let mut resolved = Vec::new();
    for t in &set.titlemap {
        let rel_entry = stem_to_entry.get(&t.stem).ok_or_else(|| {
            JobError::RomResolution(format!(
                "no gamelist entry matches titlemap stem {:?}",
                t.stem
            ))
        })?;
        let rom_path = resolve_under_root(rom_root, rel_entry)?;
        if !rom_path.is_file() {
            return Err(JobError::RomResolution(format!(
                "ROM file does not exist on NAS: {}",
                rom_path.display()
            )));
        }
        let ext = rom_path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        resolved.push(ResolvedGame {
            id: t.id.clone(),
            title: t.stem.clone(),
            rel_entry: rel_entry.clone(),
            rom_path,
            ext,
        });
    }
    Ok(resolved)
}

pub fn stage_games(
    emu: &EmulatorConfig,
    commit: &str,
    games: &[ResolvedGame],
    job_dir: &Path,
    log_line: &mut dyn FnMut(&str),
) -> JobResult<Manifest> {
    let input_root = job_dir.join("input");
    let scratch_root = job_dir.join("scratch");
    std::fs::create_dir_all(&input_root)?;

    let mut manifest_games = Vec::new();

    for g in games {
        let input_dir = input_root.join(&g.id);
        std::fs::create_dir_all(&input_dir)?;

        let container_input: String = match emu.classify_ext(&g.ext) {
            ExtKind::Direct => {
                let dest = input_dir.join(format!("game.{}", g.ext));
                stage_direct(&g.rom_path, &dest, log_line)?;
                format!("/input/{}/game.{}", g.id, g.ext)
            }
            ExtKind::Archive => {
                let scratch = scratch_root.join(&g.id);
                log_line(&format!("[{}] extracting {}", g.id, g.rom_path.display()));
                let files = archive::extract_zip(&g.rom_path, &scratch)?;
                let anchor = archive::pick_anchor(&files, &emu.supported_direct)?;

                copy_tree(&scratch, &input_dir)?;
                let anchor_rel = anchor.strip_prefix(&scratch).map_err(|_| {
                    JobError::Archive("anchor not under scratch dir".into())
                })?;
                let container_anchor = format!(
                    "/input/{}/{}",
                    g.id,
                    anchor_rel.to_string_lossy().replace('\\', "/")
                );
                log_line(&format!(
                    "[{}] anchor: {}",
                    g.id,
                    anchor_rel.to_string_lossy()
                ));
                container_anchor
            }
            ExtKind::Unsupported => {
                return Err(JobError::RomResolution(format!(
                    "[{}] extension {:?} is not supported by emulator '{}' (not in supported_direct or supported_archives)",
                    g.id, g.ext, emu.slug
                )))
            }
        };

        manifest_games.push(ManifestGame {
            id: g.id.clone(),
            title: g.title.clone(),
            input: container_input,
            output_dir: format!("/output/{}", g.id),
        });
    }

    let manifest = Manifest {
        emulator: emu.slug.clone(),
        commit: commit.to_string(),
        games: manifest_games,
    };

    let manifest_path = job_dir.join("manifest.json");
    crate::state::write_json_atomic(&manifest_path, &manifest)?;

    Ok(manifest)
}

fn stage_direct(src: &Path, dest: &Path, log_line: &mut dyn FnMut(&str)) -> JobResult<()> {
    if dest.exists() {
        std::fs::remove_file(dest).ok();
    }
    match std::fs::hard_link(src, dest) {
        Ok(()) => {
            log_line(&format!("hardlink {} -> {}", dest.display(), src.display()));
            return Ok(());
        }
        Err(e) => {
            log_line(&format!(
                "hardlink failed ({e}); copying {} -> {}",
                src.display(),
                dest.display()
            ));
        }
    }
    std::fs::copy(src, dest).map_err(|e| {
        JobError::RomResolution(format!(
            "copying {} -> {}: {e}",
            src.display(),
            dest.display()
        ))
    })?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> JobResult<()> {
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target).map_err(|e| {
                JobError::Archive(format!(
                    "copying {} -> {}: {e}",
                    entry.path().display(),
                    target.display()
                ))
            })?;
        }
    }
    Ok(())
}
