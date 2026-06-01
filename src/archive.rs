use crate::error::{JobError, JobResult};
use std::io;
use std::path::{Component, Path, PathBuf};

const ANCHOR_PRIORITY: &[&str] = &["cue", "chd", "iso", "bin", "cso", "zso", "mds", "ccd"];

/// How many times to retry a failed extraction before giving up.
const EXTRACT_RETRIES: u32 = 5;
/// Pause between extraction attempts. ROMs live on an SMB share that can
/// glitch transiently (surfacing as ENOTDIR/IO errors mid-read); waiting a
/// minute lets the mount recover before we try again.
const EXTRACT_RETRY_PAUSE: std::time::Duration = std::time::Duration::from_secs(60);

/// Extract a zip, retrying transient I/O failures (e.g. SMB hiccups) up to
/// `EXTRACT_RETRIES` times with a pause in between. Deterministic failures
/// (path escape, empty archive) fail immediately without burning retries.
pub fn extract_zip(
    zip_path: &Path,
    dest_dir: &Path,
    log_line: &mut dyn FnMut(&str),
) -> JobResult<Vec<PathBuf>> {
    let mut attempt = 0;
    loop {
        match extract_zip_once(zip_path, dest_dir) {
            Ok(files) => return Ok(files),
            Err(e) if attempt < EXTRACT_RETRIES && is_retryable(&e) => {
                attempt += 1;
                log_line(&format!(
                    "[archive] extracting {} failed ({e}); retry {}/{} in {}s",
                    zip_path.display(),
                    attempt,
                    EXTRACT_RETRIES,
                    EXTRACT_RETRY_PAUSE.as_secs()
                ));
                std::thread::sleep(EXTRACT_RETRY_PAUSE);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Logical failures that won't change on retry — don't waste minutes on them.
fn is_retryable(err: &JobError) -> bool {
    match err {
        JobError::Archive(msg) => {
            !msg.contains("escapes destination") && !msg.contains("contained no files")
        }
        _ => true,
    }
}

fn extract_zip_once(zip_path: &Path, dest_dir: &Path) -> JobResult<Vec<PathBuf>> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| JobError::Archive(format!("opening {}: {e}", zip_path.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| JobError::Archive(format!("reading zip {}: {e}", zip_path.display())))?;

    std::fs::create_dir_all(dest_dir)?;
    let mut extracted = Vec::new();

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| JobError::Archive(format!("zip entry {i}: {e}")))?;

        let raw = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        let rel = sanitize_rel_path(&raw)?;
        let out_path = dest_dir.join(&rel);

        if !out_path.starts_with(dest_dir) {
            return Err(JobError::Archive(format!(
                "zip entry {raw:?} escapes destination"
            )));
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| JobError::Archive(format!("creating {}: {e}", out_path.display())))?;
        io::copy(&mut entry, &mut out)
            .map_err(|e| JobError::Archive(format!("extracting {raw:?}: {e}")))?;
        extracted.push(out_path);
    }

    if extracted.is_empty() {
        return Err(JobError::Archive(format!(
            "archive {} contained no files",
            zip_path.display()
        )));
    }
    Ok(extracted)
}

fn sanitize_rel_path(name: &str) -> JobResult<PathBuf> {
    let normalized = name.replace('\\', "/");
    let p = Path::new(&normalized);
    if p.is_absolute() {
        return Err(JobError::Archive(format!(
            "absolute path in archive: {name:?}"
        )));
    }
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(JobError::Archive(format!(
                    "illegal path component in archive entry: {name:?}"
                )))
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(JobError::Archive(format!(
            "empty archive entry name: {name:?}"
        )));
    }
    Ok(out)
}

pub fn pick_anchor(files: &[PathBuf], supported_direct: &[String]) -> JobResult<PathBuf> {
    let ext_of = |p: &Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let supported: Vec<String> = supported_direct
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    for pref in ANCHOR_PRIORITY {
        if !supported.iter().any(|s| s == pref) {
            continue;
        }
        if let Some(p) = files.iter().find(|p| ext_of(p) == *pref) {
            return Ok(p.clone());
        }
    }

    if let Some(p) = files
        .iter()
        .find(|p| supported.iter().any(|s| *s == ext_of(p)))
    {
        return Ok(p.clone());
    }

    Err(JobError::Archive(format!(
        "no anchor file with a supported extension ({:?}) among {} extracted files",
        supported_direct,
        files.len()
    )))
}
