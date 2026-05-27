use crate::config::OutputMode;
use crate::error::{JobError, JobResult};
use std::collections::HashSet;
use std::path::Path;

const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn validate_output(output_root: &Path, ids: &[String], mode: OutputMode) -> JobResult<()> {
    if !output_root.is_dir() {
        return Err(JobError::OutputValidation(format!(
            "output dir missing: {}",
            output_root.display()
        )));
    }

    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    for entry in std::fs::read_dir(output_root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() {
            if !id_set.contains(name.as_str()) {
                return Err(JobError::OutputValidation(format!(
                    "unexpected output directory {name:?} (not a titlemap UNIQUE_ID)"
                )));
            }
            if !is_valid_id(&name) {
                return Err(JobError::OutputValidation(format!(
                    "output directory {name:?} is not a valid UNIQUE_ID"
                )));
            }
        }
    }

    for id in ids {
        let dir = output_root.join(id);
        if !dir.is_dir() {
            return Err(JobError::OutputValidation(format!(
                "missing output directory for {id}"
            )));
        }

        let mut indexes = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let fname = entry.file_name().to_string_lossy().to_string();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext != "png" {
                return Err(JobError::OutputValidation(format!(
                    "[{id}] non-png file present: {fname}"
                )));
            }
            let idx: u32 = stem.parse().map_err(|_| {
                JobError::OutputValidation(format!(
                    "[{id}] non-integer screenshot filename: {fname}"
                ))
            })?;
            check_png(&path)
                .map_err(|e| JobError::OutputValidation(format!("[{id}] {fname}: {e}")))?;
            indexes.push(idx);
        }

        indexes.sort_unstable();

        if indexes.is_empty() {
            return Err(JobError::OutputValidation(format!(
                "[{id}] no screenshots produced"
            )));
        }

        for (expected, actual) in indexes.iter().enumerate() {
            if *actual != expected as u32 {
                return Err(JobError::OutputValidation(format!(
                    "[{id}] frame indexes not contiguous from 0: got {indexes:?}"
                )));
            }
        }

        if mode == OutputMode::SinglePng && indexes != [0] {
            return Err(JobError::OutputValidation(format!(
                "[{id}] single-png mode expects exactly 0.png, got {indexes:?}"
            )));
        }
    }

    Ok(())
}

fn check_png(path: &Path) -> JobResult<()> {
    let meta = std::fs::metadata(path)?;
    if meta.len() == 0 {
        return Err(JobError::OutputValidation("empty file".into()));
    }
    let mut header = [0u8; 8];
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    f.read_exact(&mut header)
        .map_err(|_| JobError::OutputValidation("file too small for PNG header".into()))?;
    if header != PNG_MAGIC {
        return Err(JobError::OutputValidation("invalid PNG magic".into()));
    }
    Ok(())
}
