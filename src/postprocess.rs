use crate::config::{OutputMode, PostProcessConfig};
use crate::error::{JobError, JobResult};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn postprocess_output(
    output_root: &Path,
    ids: &[String],
    mode: OutputMode,
    cfg: &PostProcessConfig,
    log_line: &mut dyn FnMut(&str),
) -> JobResult<Vec<String>> {
    let mut skipped = Vec::new();
    for id in ids {
        let dir = output_root.join(id);
        if !dir.is_dir() {
            continue;
        }
        let frames = integer_png_frames(&dir)?;
        let before = frames.len();

        let kept = match mode {
            OutputMode::SinglePng => match frames.first() {
                None => Vec::new(),
                Some(first) => {
                    let blank =
                        cfg.remove_single_color && is_single_color(&first.1, cfg.solid_tolerance)?;
                    if blank {
                        Vec::new()
                    } else {
                        vec![first.clone()]
                    }
                }
            },
            OutputMode::MultiFrame => filter_frames(&frames, cfg)?,
        };

        if kept.is_empty() {
            log_line(&format!(
                "[{id}] no usable frames after post-processing ({before} captured) — skipping game"
            ));
            let _ = std::fs::remove_dir_all(&dir);
            skipped.push(id.clone());
            continue;
        }

        reindex(&dir, &kept)?;
        log_line(&format!(
            "[{id}] frames: {before} captured -> {} kept",
            kept.len()
        ));
    }
    Ok(skipped)
}

fn integer_png_frames(dir: &Path) -> JobResult<Vec<(u32, PathBuf)>> {
    let mut frames = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if let Ok(idx) = stem.parse::<u32>() {
            frames.push((idx, path));
        }
    }
    frames.sort_by_key(|(i, _)| *i);
    Ok(frames)
}

fn filter_frames(
    frames: &[(u32, PathBuf)],
    cfg: &PostProcessConfig,
) -> JobResult<Vec<(u32, PathBuf)>> {
    let mut kept: Vec<(u32, PathBuf)> = Vec::new();
    let mut prev_hash: Option<[u8; 32]> = None;

    for (idx, path) in frames {
        let img = image::open(path)
            .map_err(|e| JobError::PostProcess(format!("decoding {}: {e}", path.display())))?
            .to_rgba8();

        if cfg.remove_single_color && pixels_single_color(&img, cfg.solid_tolerance) {
            continue;
        }

        if cfg.dedupe {
            let mut hasher = Sha256::new();
            hasher.update(img.as_raw());
            let hash: [u8; 32] = hasher.finalize().into();
            if prev_hash == Some(hash) {
                continue;
            }
            prev_hash = Some(hash);
        }

        kept.push((*idx, path.clone()));
    }
    Ok(kept)
}

fn pixels_single_color(img: &image::RgbaImage, tolerance: u8) -> bool {
    let mut pixels = img.pixels();
    let Some(first) = pixels.next() else {
        return true;
    };
    let f = first.0;
    pixels.all(|p| {
        p.0.iter()
            .zip(f.iter())
            .all(|(a, b)| a.abs_diff(*b) <= tolerance)
    })
}

fn is_single_color(path: &Path, tolerance: u8) -> JobResult<bool> {
    let img = image::open(path)
        .map_err(|e| JobError::PostProcess(format!("decoding {}: {e}", path.display())))?
        .to_rgba8();
    Ok(pixels_single_color(&img, tolerance))
}

fn reindex(dir: &Path, kept: &[(u32, PathBuf)]) -> JobResult<()> {
    let mut staged = Vec::new();
    for (new_idx, (_, path)) in kept.iter().enumerate() {
        let tmp = dir.join(format!(".reindex-{new_idx}.png.tmp"));
        std::fs::rename(path, &tmp)?;
        staged.push((new_idx, tmp));
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("png") {
            std::fs::remove_file(p)?;
        }
    }

    for (new_idx, tmp) in staged {
        let final_path = dir.join(format!("{new_idx}.png"));
        std::fs::rename(&tmp, &final_path)?;
    }
    Ok(())
}
