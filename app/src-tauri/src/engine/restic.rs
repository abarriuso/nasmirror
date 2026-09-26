//! Alternative engine to robocopy: wraps the `restic` binary, which must be on
//! the PATH (`winget install restic.restic`). It adds what robocopy cannot:
//! block-level deduplication, AES-256 encryption of the repository and
//! versioned snapshots (each backup is a new version, not an overwrite).
//!
//! The job destination is used as the path of a local restic repository
//! (`RESTIC_REPOSITORY=<destination>`); if the destination is a mounted
//! `\\nas\share`, the repository simply lives there.

use std::process::Stdio;

use serde_json::Value;
use tokio::process::{Child, Command};

use super::types::{AdvancedOptions, ResticSummary};

fn base_command(repo: &str, password: &str) -> Command {
    let mut cmd = Command::new("restic");
    cmd.env("RESTIC_REPOSITORY", repo);
    cmd.env("RESTIC_PASSWORD", password);
    cmd.stdin(Stdio::null());
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Debug, thiserror::Error)]
pub enum ResticError {
    #[error("could not find 'restic' in your PATH. Install it with 'winget install restic.restic' and restart the app")]
    NotFound,
    #[error("wrong password for the restic repository")]
    WrongPassword,
    #[error("restic failed: {0}")]
    Failed(String),
}

fn map_spawn_error(e: std::io::Error) -> ResticError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ResticError::NotFound
    } else {
        ResticError::Failed(e.to_string())
    }
}

pub async fn is_initialized(repo: &str, password: &str) -> Result<bool, ResticError> {
    let mut cmd = base_command(repo, password);
    cmd.args(["cat", "config"]);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());
    let output = cmd.output().await.map_err(map_spawn_error)?;
    classify_cat_config(
        output.status.code(),
        &String::from_utf8_lossy(&output.stderr),
    )
}

/// Reads the result of `restic cat config`. Only "the repository does not
/// exist" may lead to `restic init`: a wrong password or an unreachable
/// repository would otherwise end in a misleading "config file already
/// exists" from `init`. Exit codes 10 and 12 exist since restic 0.17; older
/// versions exit with 1 and are recognised by their message.
fn classify_cat_config(code: Option<i32>, stderr: &str) -> Result<bool, ResticError> {
    match code {
        Some(0) => Ok(true),
        Some(10) => Ok(false),
        Some(12) => Err(ResticError::WrongPassword),
        _ if stderr.contains("Is there a repository at the following location?") => Ok(false),
        _ if stderr.contains("wrong password") => Err(ResticError::WrongPassword),
        _ => Err(ResticError::Failed(
            stderr
                .lines()
                .map(str::trim)
                .find(|l| l.starts_with("Fatal:"))
                .or_else(|| stderr.lines().map(str::trim).rfind(|l| !l.is_empty()))
                .map(str::to_string)
                .unwrap_or_else(|| format!("restic exited with code {code:?}")),
        )),
    }
}

pub async fn init_repo(repo: &str, password: &str) -> Result<(), ResticError> {
    let mut cmd = base_command(repo, password);
    cmd.args(["init"]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let output = cmd.output().await.map_err(map_spawn_error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ResticError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// `restic forget --prune`: applies the retention policy (how many daily,
/// weekly and monthly snapshots to keep) and deletes the data of snapshots
/// that fall outside it.
pub async fn forget_and_prune(
    repo: &str,
    password: &str,
    keep_daily: u32,
    keep_weekly: u32,
    keep_monthly: u32,
) -> Result<(), ResticError> {
    if keep_daily == 0 && keep_weekly == 0 && keep_monthly == 0 {
        return Ok(()); // no retention policy configured: delete nothing
    }
    let mut cmd = base_command(repo, password);
    cmd.arg("forget").arg("--prune");
    if keep_daily > 0 {
        cmd.arg("--keep-daily").arg(keep_daily.to_string());
    }
    if keep_weekly > 0 {
        cmd.arg("--keep-weekly").arg(keep_weekly.to_string());
    }
    if keep_monthly > 0 {
        cmd.arg("--keep-monthly").arg(keep_monthly.to_string());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let output = cmd.output().await.map_err(map_spawn_error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ResticError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

pub fn start_backup(
    source: &str,
    repo: &str,
    password: &str,
    adv: &AdvancedOptions,
    dry_run: bool,
) -> Result<Child, ResticError> {
    let mut cmd = base_command(repo, password);
    cmd.arg("backup").arg(source).arg("--json");
    if dry_run {
        cmd.arg("--dry-run");
    }
    for d in &adv.exclude_dirs {
        if !d.is_empty() {
            cmd.arg("--exclude").arg(d);
        }
    }
    for f in &adv.exclude_files {
        if !f.is_empty() {
            cmd.arg("--exclude").arg(f);
        }
    }
    cmd.stdout(Stdio::piped());
    // stderr is drained by a separate task (see `run_restic_job`); otherwise
    // restic would block once the pipe buffer fills up.
    cmd.stderr(Stdio::piped());
    cmd.spawn().map_err(map_spawn_error)
}

/// How `restic backup` ended, from its exit code (restic docs, "Exit status
/// codes"): 0 = snapshot created; 3 = snapshot created, but some source files
/// could not be read, so it is incomplete; anything else = no snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupExit {
    Complete,
    Incomplete,
    Failed,
}

pub fn classify_backup_exit(code: Option<i32>) -> BackupExit {
    match code {
        Some(0) => BackupExit::Complete,
        Some(3) => BackupExit::Incomplete,
        _ => BackupExit::Failed,
    }
}

pub struct StatusUpdate {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub current_file: String,
}

pub enum Event {
    Status(StatusUpdate),
    Summary(ResticSummary),
    /// A line in an unexpected format, a per-file error, or anything else that
    /// does not change the result: written to the log as-is, ignored by the UI.
    Other,
}

/// `restic backup --json` prints one JSON object per line (`message_type`:
/// "status" while copying, "summary" at the end). Fields are read loosely
/// rather than with a strict serde schema, so a field that is missing or
/// renamed in another restic version reads as 0/empty instead of discarding
/// the whole line.
pub fn parse_line(line: &str) -> Event {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Event::Other;
    };
    let u = |key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
    let f = |key: &str| v.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let s = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    match v.get("message_type").and_then(Value::as_str) {
        Some("status") => {
            // Some restic versions omit `bytes_done` in the first status
            // lines; estimate it from `percent_done` instead.
            let total_bytes = u("total_bytes");
            let bytes_done = v
                .get("bytes_done")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| (f("percent_done") * total_bytes as f64) as u64);
            let current_file = v
                .get("current_files")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Event::Status(StatusUpdate {
                bytes_done,
                bytes_total: total_bytes,
                files_done: u("files_done"),
                files_total: u("total_files"),
                current_file,
            })
        }
        Some("summary") => Event::Summary(ResticSummary {
            snapshot_id: s("snapshot_id"),
            files_new: u("files_new"),
            files_changed: u("files_changed"),
            files_unmodified: u("files_unmodified"),
            data_added: u("data_added"),
            total_files_processed: u("total_files_processed"),
            total_bytes_processed: u("total_bytes_processed"),
        }),
        _ => Event::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_line() {
        let line = r#"{"message_type":"status","percent_done":0.5,"total_files":10,"files_done":5,"total_bytes":2000,"bytes_done":1000,"current_files":["C:\\a.txt"]}"#;
        match parse_line(line) {
            Event::Status(s) => {
                assert_eq!(s.bytes_done, 1000);
                assert_eq!(s.bytes_total, 2000);
                assert_eq!(s.files_done, 5);
                assert_eq!(s.files_total, 10);
                assert_eq!(s.current_file, "C:\\a.txt");
            }
            _ => panic!("expected a status line"),
        }
    }

    #[test]
    fn status_without_bytes_done_uses_percent() {
        let line = r#"{"message_type":"status","percent_done":0.25,"total_bytes":4000}"#;
        match parse_line(line) {
            Event::Status(s) => assert_eq!(s.bytes_done, 1000),
            _ => panic!("expected a status line"),
        }
    }

    #[test]
    fn parse_summary_line() {
        let line = r#"{"message_type":"summary","files_new":3,"files_changed":1,"files_unmodified":7,"data_added":512,"total_files_processed":11,"total_bytes_processed":9000,"snapshot_id":"abc123"}"#;
        match parse_line(line) {
            Event::Summary(s) => {
                assert_eq!(s.snapshot_id, "abc123");
                assert_eq!(s.files_new, 3);
                assert_eq!(s.files_changed, 1);
                assert_eq!(s.data_added, 512);
            }
            _ => panic!("expected a summary line"),
        }
    }

    /// Exit codes and messages checked against a real restic 0.19.1.
    #[test]
    fn cat_config_outcomes() {
        assert!(matches!(classify_cat_config(Some(0), ""), Ok(true)));
        assert!(matches!(
            classify_cat_config(
                Some(10),
                "Fatal: repository does not exist: unable to open config file\n\
                 Is there a repository at the following location?\nrepo"
            ),
            Ok(false)
        ));
        assert!(matches!(
            classify_cat_config(Some(12), "Fatal: wrong password or no key found"),
            Err(ResticError::WrongPassword)
        ));
        // restic < 0.17 exits with 1 for everything.
        assert!(matches!(
            classify_cat_config(Some(1), "Is there a repository at the following location?"),
            Ok(false)
        ));
        assert!(matches!(
            classify_cat_config(Some(1), "Fatal: wrong password or no key found"),
            Err(ResticError::WrongPassword)
        ));
        match classify_cat_config(Some(11), "Fatal: unable to create lock in backend: repository is already locked") {
            Err(ResticError::Failed(msg)) => assert!(msg.contains("already locked")),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn backup_exit_codes() {
        assert_eq!(classify_backup_exit(Some(0)), BackupExit::Complete);
        // Unreadable source files: the snapshot exists, but is incomplete.
        assert_eq!(classify_backup_exit(Some(3)), BackupExit::Incomplete);
        assert_eq!(classify_backup_exit(Some(1)), BackupExit::Failed);
        assert_eq!(classify_backup_exit(Some(12)), BackupExit::Failed);
        assert_eq!(classify_backup_exit(Some(130)), BackupExit::Failed);
        // Killed, no exit code.
        assert_eq!(classify_backup_exit(None), BackupExit::Failed);
    }

    #[test]
    fn parse_other() {
        assert!(matches!(parse_line("not json"), Event::Other));
        assert!(matches!(
            parse_line(r#"{"message_type":"error"}"#),
            Event::Other
        ));
    }
}
