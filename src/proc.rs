use crate::error::{JobError, JobResult};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

/// Like [`run_checked`], but if `timeout` elapses before the process exits, the
/// container named `kill_container` is force-stopped via `docker kill` and the
/// call fails instead of blocking forever.
#[allow(clippy::too_many_arguments)]
pub fn run_checked_with_timeout(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    log_path: &Path,
    redact: &[String],
    err: impl Fn(String) -> JobError,
    timeout: Duration,
    kill_container: &str,
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
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| JobError::Other(format!("spawning '{program}': {e}")))?;

    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let timed_out = Arc::new(AtomicBool::new(false));
    let watcher = {
        let timed_out = Arc::clone(&timed_out);
        let container = kill_container.to_string();
        std::thread::spawn(move || {
            // Wakes immediately when the main thread signals completion;
            // only a real timeout (RecvTimeoutError::Timeout) kills the container.
            if done_rx.recv_timeout(timeout).is_err() {
                timed_out.store(true, Ordering::SeqCst);
                let _ = Command::new("docker").args(["kill", &container]).status();
            }
        })
    };

    let output = child
        .wait_with_output()
        .map_err(|e| JobError::Other(format!("waiting for '{program}': {e}")));
    let _ = done_tx.send(());
    let _ = watcher.join();
    let output = output?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status = output.status.code().unwrap_or(-1);

    if !stdout.is_empty() {
        writeln!(log, "{}", redact_str(&stdout, redact))?;
    }
    if !stderr.is_empty() {
        writeln!(log, "[stderr]\n{}", redact_str(&stderr, redact))?;
    }
    if timed_out.load(Ordering::SeqCst) {
        writeln!(
            log,
            "[timeout] killed after {}s, exit {status}\n",
            timeout.as_secs()
        )?;
        return Err(err(format!(
            "`{program}` timed out after {}s and was killed (see {})",
            timeout.as_secs(),
            log_path.display()
        )));
    }
    writeln!(log, "[exit] {status}\n")?;

    if status != 0 {
        let tail = redact_str(&stderr, redact);
        return Err(err(format!(
            "`{program}` exited {status} (see {}): {}",
            log_path.display(),
            tail.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
        )));
    }

    Ok(CmdOutput {
        status,
        stdout,
        stderr,
    })
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
