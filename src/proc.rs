use crate::error::{JobError, JobResult};
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub struct CmdOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

fn render_cmdline(program: &str, args: &[String]) -> String {
    let mut s = String::from(program);
    for a in args {
        s.push(' ');
        if a.contains(' ') {
            s.push('"');
            s.push_str(a);
            s.push('"');
        } else {
            s.push_str(a);
        }
    }
    s
}

pub fn run_logged(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    log_path: &Path,
    redact: &[String],
) -> JobResult<CmdOutput> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let header = redact_str(&render_cmdline(program, args), redact);
    writeln!(log, "$ {header}")?;

    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd
        .output()
        .map_err(|e| JobError::Other(format!("spawning '{program}': {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status = output.status.code().unwrap_or(-1);

    if !stdout.is_empty() {
        writeln!(log, "{}", redact_str(&stdout, redact))?;
    }
    if !stderr.is_empty() {
        writeln!(log, "[stderr]\n{}", redact_str(&stderr, redact))?;
    }
    writeln!(log, "[exit] {status}\n")?;

    Ok(CmdOutput {
        status,
        stdout,
        stderr,
    })
}

pub fn run_checked(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    log_path: &Path,
    redact: &[String],
    err: impl Fn(String) -> JobError,
) -> JobResult<CmdOutput> {
    let out = run_logged(program, args, cwd, log_path, redact)?;
    if out.status != 0 {
        let tail = redact_str(&out.stderr, redact);
        return Err(err(format!(
            "`{program}` exited {} (see {}): {}",
            out.status,
            log_path.display(),
            tail.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
        )));
    }
    Ok(out)
}

fn redact_str(s: &str, redact: &[String]) -> String {
    let mut out = s.to_string();
    for r in redact {
        if !r.is_empty() {
            out = out.replace(r, "***");
        }
    }
    out
}
