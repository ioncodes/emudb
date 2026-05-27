use crate::error::{JobError, JobResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TitleEntry {
    pub id: String,

    pub stem: String,
}

#[derive(Debug, Clone)]
pub struct GameSet {
    pub gamelist: Vec<String>,
    pub titlemap: Vec<TitleEntry>,
}

const ID_RE: &str = r"^[A-Za-z0-9_-]+$";

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn read_lines(path: &Path) -> JobResult<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| JobError::GameSet(format!("reading {}: {e}", path.display())))?;
    Ok(text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect())
}

pub fn read_gamelist(path: &Path) -> JobResult<Vec<String>> {
    read_lines(path)
}

pub fn read_titlemap(path: &Path) -> JobResult<Vec<TitleEntry>> {
    let mut entries = Vec::new();
    for line in read_lines(path)? {
        let (id, stem) = line
            .split_once('=')
            .ok_or_else(|| JobError::GameSet(format!("titlemap line missing '=': {line:?}")))?;
        let id = id.trim().to_string();
        let stem = stem.trim().to_string();
        if !is_valid_id(&id) {
            return Err(JobError::GameSet(format!(
                "UNIQUE_ID {id:?} does not match {ID_RE}"
            )));
        }
        if stem.is_empty() {
            return Err(JobError::GameSet(format!(
                "titlemap entry {id:?} has empty filename/title"
            )));
        }
        entries.push(TitleEntry { id, stem });
    }
    Ok(entries)
}

pub fn entry_stem(entry: &str) -> Option<String> {
    Path::new(entry)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
}

pub fn validate_game_set(set: &GameSet) -> JobResult<()> {
    let mut seen_ids = HashMap::new();
    for e in &set.titlemap {
        if seen_ids.insert(e.id.clone(), ()).is_some() {
            return Err(JobError::GameSet(format!("duplicate UNIQUE_ID {:?}", e.id)));
        }
    }

    let mut stem_to_entries: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &set.gamelist {
        let stem = entry_stem(entry).ok_or_else(|| {
            JobError::GameSet(format!("gamelist entry has no file stem: {entry:?}"))
        })?;
        stem_to_entries.entry(stem).or_default().push(entry.clone());
    }

    let mut referenced: HashMap<String, u32> = HashMap::new();
    for e in &set.titlemap {
        match stem_to_entries.get(&e.stem) {
            None => {
                return Err(JobError::GameSet(format!(
                    "titlemap entry {:?} (stem {:?}) has no matching gamelist file",
                    e.id, e.stem
                )))
            }
            Some(matches) if matches.len() > 1 => {
                return Err(JobError::GameSet(format!(
                    "stem {:?} is ambiguous: {} gamelist entries share it ({:?})",
                    e.stem,
                    matches.len(),
                    matches
                )))
            }
            Some(_) => {
                *referenced.entry(e.stem.clone()).or_default() += 1;
            }
        }
    }

    for entry in &set.gamelist {
        let stem = entry_stem(entry).unwrap();
        match referenced.get(&stem).copied().unwrap_or(0) {
            0 => {
                return Err(JobError::GameSet(format!(
                    "gamelist entry {entry:?} is not referenced by any titlemap value"
                )))
            }
            1 => {}
            n => {
                return Err(JobError::GameSet(format!(
                    "gamelist entry {entry:?} (stem {stem:?}) is referenced by {n} titlemap values"
                )))
            }
        }
    }

    Ok(())
}

pub fn load_and_stage_game_set(repo: &Path, job_dir: &Path) -> JobResult<GameSet> {
    let gamelist_src = repo.join("gamelist.txt");
    let titlemap_src = repo.join("titlemap.txt");

    let set = GameSet {
        gamelist: read_gamelist(&gamelist_src)?,
        titlemap: read_titlemap(&titlemap_src)?,
    };
    validate_game_set(&set)?;

    copy_into(&gamelist_src, &job_dir.join("gamelist.txt"))?;
    copy_into(&titlemap_src, &job_dir.join("titlemap.txt"))?;

    Ok(set)
}

fn copy_into(src: &Path, dst: &PathBuf) -> JobResult<()> {
    std::fs::copy(src, dst).map_err(|e| {
        JobError::GameSet(format!(
            "copying {} -> {}: {e}",
            src.display(),
            dst.display()
        ))
    })?;
    Ok(())
}
