use crate::archive;
use crate::config::{EmulatorConfig, ExtKind};
use crate::error::{JobError, JobResult};
use crate::screenshotter::{entry_stem, GameSet};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedGame {
    pub id: String,

    #[allow(dead_code)]
    pub title: String,

    #[allow(dead_code)]
    pub rel_entry: String,

    pub rom_path: PathBuf,
    pub ext: String,
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

pub fn stage_one_game(
    emu: &EmulatorConfig,
    game: &ResolvedGame,
    job_id: &str,
    staging_root: &Path,
    log_line: &mut dyn FnMut(&str),
) -> JobResult<String> {
    let game_staging = staging_root.join(job_id).join(&game.id);
    std::fs::create_dir_all(&game_staging)?;

    let container_input: String = match emu.classify_ext(&game.ext) {
        ExtKind::Direct => {
            let dest = game_staging.join(format!("game.{}", game.ext));
            stage_direct(&game.rom_path, &dest, log_line)?;
            format!("/staging/{}/{}/game.{}", job_id, game.id, game.ext)
        }
        ExtKind::Archive => {
            log_line(&format!(
                "[{}] extracting {}",
                game.id,
                game.rom_path.display()
            ));
            let files = archive::extract_zip(&game.rom_path, &game_staging)?;
            let anchor = archive::pick_anchor(&files, &emu.supported_direct)?;
            let anchor_rel = anchor
                .strip_prefix(&game_staging)
                .map_err(|_| JobError::Archive("anchor not under staging dir".into()))?;
            let container_anchor = format!(
                "/staging/{}/{}/{}",
                job_id,
                game.id,
                anchor_rel.to_string_lossy().replace('\\', "/")
            );
            log_line(&format!(
                "[{}] anchor: {}",
                game.id,
                anchor_rel.to_string_lossy()
            ));
            container_anchor
        }
        ExtKind::Unsupported => {
            return Err(JobError::RomResolution(format!(
                "[{}] extension {:?} is not supported by emulator '{}' (not in supported_direct or supported_archives)",
                game.id, game.ext, emu.slug
            )))
        }
    };

    Ok(container_input)
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
