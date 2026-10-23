pub mod children;
pub mod console;
pub mod netuse;
pub mod restic;
pub mod robocopy;
pub mod types;
pub mod wol;

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, BufReader};

use types::{
    Engine, EngineSummary, JobEvent, JobPhase, JobRequest, JobResult,
    Outcome, Profile, ProgressSample, ResticSummary, RobocopySummary,
};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("the source folder does not exist: {0}")]
    SourceMissing(String),
    #[error("choose a destination folder")]
    DestinationEmpty,
    #[error("\"{0}\" is not a full path: write it with its drive (D:\\Backups) or as a network share (\\\\nas\\share)")]
    NotAbsolute(String),
    #[error("source and destination are the same folder")]
    SameSourceAndDestination,
    #[error("the destination is inside the source folder: that would copy forever")]
    DestinationInsideSource,
    #[error("the source is inside the destination folder: mirror mode would delete it")]
    SourceInsideDestination,
}

fn normalize(p: &str) -> String {
    p.trim()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
        .replace('/', "\\")
}

/// Resolves `.` and `..` without touching the disk. `PathBuf::pop` at the root
/// returns false, so an extra `..` stays at the root instead of escaping it.
fn lexical_normalize(path: &str) -> String {
    let raw = path.trim().replace('/', "\\");
    let mut out = PathBuf::new();
    for comp in Path::new(&raw).components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.display().to_string()
}

/// Strips the verbatim prefix (`\\?\`) that `canonicalize` adds on Windows,
/// so the result is still a regular, comparable path.
fn strip_verbatim(path: &Path) -> String {
    let s = path.display().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

/// Puts a path in canonical form so it can be compared with another: resolves
/// `.` and `..`, then canonicalizes the part that already exists on disk
/// (resolving symlinks, junctions and 8.3 short names). Without this,
/// `C:\source\sub\..\..\source` would not be recognised as the source itself
/// and would slip past the checks in `validate_paths`.
fn resolve_for_compare(path: &str) -> String {
    let lex = lexical_normalize(path);
    // Network paths intentionally get lexical normalization only:
    // `validate_paths` runs before Wake-on-LAN, and touching a NAS that is
    // still asleep would block until SMB hits its own timeout.
    if lex.starts_with(r"\\") {
        return normalize(&lex);
    }
    let mut head = PathBuf::from(&lex);
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canon) = head.canonicalize() {
            let mut out = PathBuf::from(strip_verbatim(&canon));
            for part in tail.iter().rev() {
                out.push(part);
            }
            return normalize(&out.display().to_string());
        }
        // `lex` has no `..` components left, so `file_name` only fails at the
        // root or drive prefix: stop there.
        let Some(name) = head.file_name().map(|n| n.to_os_string()) else {
            break;
        };
        let Some(parent) = head.parent().map(Path::to_path_buf) else {
            break;
        };
        tail.push(name);
        head = parent;
    }
    normalize(&lex)
}

/// Sanity checks before starting a potentially destructive copy: identical
/// source and destination, or one folder nested inside the other. If the
/// source lives inside the destination, `/MIR` sees the source folder as an
/// "extra" in the destination and deletes it.
///
/// A network source is not checked for existence here: this runs before
/// Wake-on-LAN and before the share is connected with the job's credentials,
/// so touching it could block on a sleeping NAS or fail for lack of a login.
/// `run_job` checks it once the share is reachable.
pub fn validate_paths(source: &str, destination: &str) -> Result<(), PathError> {
    if destination.trim().is_empty() {
        return Err(PathError::DestinationEmpty);
    }
    for path in [source, destination] {
        if !is_absolute(path) {
            return Err(PathError::NotAbsolute(path.trim().to_string()));
        }
    }
    if !is_network_path(source) && !Path::new(source).is_dir() {
        return Err(PathError::SourceMissing(source.to_string()));
    }
    let src_n = resolve_for_compare(source);
    let dst_n = resolve_for_compare(destination);
    if src_n == dst_n {
        return Err(PathError::SameSourceAndDestination);
    }
    if dst_n.starts_with(&format!("{src_n}\\")) {
        return Err(PathError::DestinationInsideSource);
    }
    if src_n.starts_with(&format!("{dst_n}\\")) {
        return Err(PathError::SourceInsideDestination);
    }
    Ok(())
}

fn is_network_path(path: &str) -> bool {
    let p = path.trim_start();
    p.starts_with(r"\\") || p.starts_with("//")
}

/// `D:\folder` or `\\host\share\folder`. Anything else is resolved by Windows
/// against something the user never typed: `\nas\share` (one backslash short)
/// against the root of the current drive, `backup` or `D:backup` against the
/// app's working folder. In mirror mode that is a copy, and deletions, in an
/// unexpected place.
fn is_absolute(path: &str) -> bool {
    if is_network_path(path) {
        return share_root(path).is_some();
    }
    let p = path.trim().as_bytes();
    p.len() >= 3 && p[0].is_ascii_alphabetic() && p[1] == b':' && matches!(p[2], b'\\' | b'/')
}

/// `\\host\share` of a network path. Accepts the same spellings as
/// `is_network_path` (`//host/share`, surrounding spaces), so every path it
/// treats as network also gets its share connected and its host woken.
fn share_root(path: &str) -> Option<String> {
    let path = path.trim().replace('/', "\\");
    let rest = path.strip_prefix(r"\\")?;
    let mut parts = rest.splitn(3, '\\');
    let host = parts.next()?;
    let share = parts.next()?;
    if host.is_empty() || share.is_empty() {
        return None;
    }
    Some(format!("\\\\{host}\\{share}"))
}

pub struct JobContext {
    pub job_id: String,
    pub cancel: Arc<AtomicBool>,
}

impl JobContext {
    pub fn new(job_id: String) -> Self {
        Self {
            job_id,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Runs a job end to end, reporting typed events through `emit`. Callable
/// both from a Tauri command (GUI) and from the unattended `--job` mode used
/// by scheduled tasks.
pub async fn run_job(
    ctx: &JobContext,
    req: JobRequest,
    log_dir: &Path,
    emit: impl Fn(JobEvent) + Send + Sync + 'static,
) -> JobResult {
    let started = Instant::now();
    let job_id = ctx.job_id.clone();
    let mut profile = req.profile;
    // Paths come as typed in the form: stray spaces around them would make a
    // local folder "not exist" and hide a network share from `share_root`.
    profile.source = profile.source.trim().to_string();
    profile.destination = profile.destination.trim().to_string();
    let dry_run = req.dry_run;

    macro_rules! finish {
        ($outcome:expr, $summary:expr, $error:expr, $log_path:expr) => {{
            let result = JobResult {
                outcome: $outcome,
                summary: $summary,
                error: $error,
                log_path: $log_path,
                elapsed_secs: started.elapsed().as_secs_f64(),
            };
            // Dry runs copy nothing, so they are not recorded in history.
            if !dry_run {
                crate::history::record(Path::new(&result.log_path), &profile, &result);
            }
            emit(JobEvent::Finished {
                job_id: job_id.clone(),
                result: result.clone(),
            });
            return result;
        }};
    }

    // The log path is picked before validating, so a run rejected here still
    // leaves a history entry: in unattended mode that is the only place the
    // reason shows up.
    let log_path = robocopy::default_log_path(log_dir);
    let _ = std::fs::create_dir_all(log_dir);

    let phase = |phase: JobPhase, detail: String| {
        emit(JobEvent::Phase {
            job_id: job_id.clone(),
            phase,
            detail: Some(detail),
        })
    };

    phase(
        JobPhase::Checking,
        format!("{} → {}", profile.source, profile.destination),
    );
    if let Err(e) = validate_paths(&profile.source, &profile.destination) {
        finish!(
            Outcome::Failed,
            None,
            Some(e.to_string()),
            log_path.display().to_string()
        );
    }

    // ── Wake-on-LAN ──────────────────────────────────────────────
    if let Some(wol) = &profile.wake_on_lan {
        let host = if wol.host.is_empty() {
            share_root(&profile.destination)
                .and_then(|s| wol::guess_host_from_share(&s))
                .unwrap_or_default()
        } else {
            wol.host.clone()
        };
        phase(
            JobPhase::WakingTarget,
            if host.is_empty() {
                format!("magic packet to {}; no host to check, so a fixed 12 s wait", wol.mac)
            } else {
                format!(
                    "magic packet to {}, then up to {} s for {host} to answer on port 445 (SMB)",
                    wol.mac, wol.timeout_secs
                )
            },
        );
        if let Err(e) = wol::wake_and_wait(&wol.mac, &host, wol.timeout_secs, ctx.cancel.clone()).await {
            let outcome = if matches!(e, wol::WolError::Cancelled) {
                Outcome::Cancelled
            } else {
                Outcome::ConnectionError
            };
            finish!(
                outcome,
                None,
                Some(e.to_string()),
                log_path.display().to_string()
            );
        }
    }

    // ── Network share connection (if needed) ────────────────────
    let share = if profile.credentials.is_some() {
        share_root(&profile.destination).or_else(|| share_root(&profile.source))
    } else {
        None
    };
    if let (Some(share), Some(creds)) = (&share, &profile.credentials) {
        phase(JobPhase::Connecting, format!("{share} as {}", creds.user));
        let share_for_msg = share.clone();
        let share = share.clone();
        let user = creds.user.clone();
        let password = req.password.clone().unwrap_or_default();
        let connect_task =
            tokio::task::spawn_blocking(move || netuse::connect(&share, &user, &password));
        // WNetAddConnection2W has no timeout of its own: if the NAS does not
        // respond (powered off, network down, stuck SMB negotiation) it can
        // block indefinitely, leaving the job busy with no result and the UI
        // unable to recover without restarting the app.
        match tokio::time::timeout(std::time::Duration::from_secs(20), connect_task).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => {
                finish!(
                    Outcome::ConnectionError,
                    None,
                    Some(e.to_string()),
                    log_path.display().to_string()
                );
            }
            Ok(Err(e)) => {
                finish!(
                    Outcome::ConnectionError,
                    None,
                    Some(format!("internal error while connecting: {e}")),
                    log_path.display().to_string()
                );
            }
            Err(_) => {
                finish!(
                    Outcome::ConnectionError,
                    None,
                    Some(format!(
                        "no response from {share_for_msg} after 20 s: check that the NAS is powered on and reachable on the network"
                    )),
                    log_path.display().to_string()
                );
            }
        }
    }

    // A network source can only be checked now that the NAS is awake and the
    // share connected (see `validate_paths`).
    if is_network_path(&profile.source) && !Path::new(&profile.source).is_dir() {
        if let Some(share) = &share {
            netuse::disconnect(share);
        }
        finish!(
            Outcome::Failed,
            None,
            Some(PathError::SourceMissing(profile.source.clone()).to_string()),
            log_path.display().to_string()
        );
    }

    if profile.engine == Engine::Restic {
        let restic_password = req.restic_password.clone();
        let result = run_restic_job(
            &ctx.job_id,
            &profile,
            restic_password,
            req.dry_run,
            &log_path,
            ctx.cancel.clone(),
            &emit,
        )
        .await;
        if let Some(share) = &share {
            netuse::disconnect(share);
        }
        // Elapsed time counts from job start (including WoL and connection),
        // as on the robocopy path.
        let result = JobResult {
            elapsed_secs: started.elapsed().as_secs_f64(),
            ..result
        };
        if !dry_run {
            crate::history::record(Path::new(&result.log_path), &profile, &result);
        }
        emit(JobEvent::Finished {
            job_id: job_id.clone(),
            result: result.clone(),
        });
        return result;
    }

    // ── Scan (list-only pass, /L) ────────────────────────────────
    // A copy confirmed from the preview reuses what the preview found: its
    // numbers only drive the progress bar, and robocopy itself decides what
    // to copy.
    let preview = req.preview.filter(|_| !dry_run);
    let scan = if let Some(totals) = preview {
        robocopy::ScanOutcome {
            totals,
            warning: None,
        }
    } else {
        phase(
            JobPhase::Scanning,
            robocopy::scan_command_line(
                &profile.source,
                &profile.destination,
                profile.mode,
                &profile.advanced,
            ),
        );
        match robocopy::scan(
            &profile.source,
            &profile.destination,
            profile.mode,
            &profile.advanced,
            ctx.cancel.clone(),
            |dirs_scanned, current_dir, totals| {
                emit(JobEvent::ScanProgress {
                    job_id: job_id.clone(),
                    dirs_scanned,
                    current_dir: current_dir.to_string(),
                    totals,
                })
            },
        )
        .await
        {
            Ok(Some(s)) => s,
            Ok(None) => {
                if let Some(share) = &share {
                    netuse::disconnect(share);
                }
                finish!(
                    Outcome::Cancelled,
                    None,
                    None,
                    log_path.display().to_string()
                );
            }
            Err(e) => {
                if let Some(share) = &share {
                    netuse::disconnect(share);
                }
                finish!(
                    Outcome::Failed,
                    None,
                    Some(format!("could not scan the copy: {e}")),
                    log_path.display().to_string()
                );
            }
        }
    };
    emit(JobEvent::ScanResult {
        job_id: job_id.clone(),
        totals: scan.totals,
        warning: scan.warning.clone(),
    });

    if scan.totals.is_empty() {
        if let Some(share) = &share {
            netuse::disconnect(share);
        }
        // Nothing to do in what could be read: a problem robocopy reported
        // (an unreadable folder, say) is still a failure, as it would be in
        // the copy.
        let outcome = if scan.warning.is_some() {
            Outcome::Failed
        } else {
            Outcome::NoChanges
        };
        finish!(
            outcome,
            None,
            scan.warning.clone(),
            log_path.display().to_string()
        );
    }

    if req.dry_run {
        if let Some(share) = &share {
            netuse::disconnect(share);
        }
        // `--dry-run` has no window to show the scan's warning in.
        finish!(
            Outcome::Success,
            None,
            scan.warning.clone(),
            log_path.display().to_string()
        );
    }

    // ── Copy ─────────────────────────────────────────────────────
    phase(
        JobPhase::Copying,
        robocopy::copy_command_line(
            &profile.source,
            &profile.destination,
            profile.mode,
            &profile.advanced,
        ),
    );
    let outcome = run_copy(
        &job_id,
        &profile,
        &scan,
        &log_path,
        ctx.cancel.clone(),
        &emit,
    )
    .await;

    if let Some(share) = &share {
        netuse::disconnect(share);
    }

    match outcome {
        Ok((exit_code, summary, errors)) => {
            let summary = Some(EngineSummary::Robocopy(summary));
            if ctx.cancel.load(Ordering::SeqCst) {
                finish!(
                    Outcome::Cancelled,
                    summary,
                    None,
                    log_path.display().to_string()
                );
            }
            let result_outcome = classify_exit_code(exit_code);
            // Robocopy explains failures only in its output: without this the
            // window would just say "failed" and leave the reason in the log.
            let error = (result_outcome == Outcome::Failed).then(|| {
                errors.unwrap_or_else(|| {
                    format!("robocopy exited with code {exit_code}; the log has the details")
                })
            });
            finish!(
                result_outcome,
                summary,
                error,
                log_path.display().to_string()
            );
        }
        Err(e) => {
            finish!(
                Outcome::Failed,
                None,
                Some(e),
                log_path.display().to_string()
            );
        }
    }
}

/// Robocopy exit codes are a bitmask: 0 = nothing to do, 1 = files copied,
/// 2 = extra files detected, 4 = mismatches, 8 = some copies failed,
/// 16 = fatal error. Bits 8 and 16 are real failures.
fn classify_exit_code(code: i32) -> Outcome {
    if code & 16 != 0 || code & 8 != 0 {
        Outcome::Failed
    } else if code & 4 != 0 {
        Outcome::SuccessWithMismatches
    } else {
        Outcome::Success
    }
}

const PROGRESS_SAMPLE_INTERVAL_MS: u128 = 200;

async fn run_copy(
    job_id: &str,
    profile: &Profile,
    scan: &robocopy::ScanOutcome,
    log_path: &Path,
    cancel: Arc<AtomicBool>,
    emit: &(impl Fn(JobEvent) + Send + Sync + 'static),
) -> Result<(i32, RobocopySummary, Option<String>), String> {
    let mut child = robocopy::start_copy(
        &profile.source,
        &profile.destination,
        profile.mode,
        &profile.advanced,
    )
    .map_err(|e| format!("could not start robocopy: {e}"))?;

    let stdout = child.stdout.take().ok_or("robocopy produced no output")?;
    let mut lines = console::ConsoleLines::new(stdout);
    let parser = robocopy::LineParser::new(&profile.destination);

    let mut log_file = std::fs::File::create(log_path)
        .map(std::io::BufWriter::new)
        .map_err(|e| format!("could not create the log file: {e}"))?;
    let _ = writeln!(
        log_file,
        "START: {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    let _ = writeln!(log_file, "SRC  : {}", profile.source);
    let _ = writeln!(log_file, "DST  : {}", profile.destination);
    let _ = writeln!(log_file, "MODE : {}", profile.mode.robocopy_flag());
    let _ = writeln!(
        log_file,
        "CMD  : {}",
        robocopy::copy_command_line(
            &profile.source,
            &profile.destination,
            profile.mode,
            &profile.advanced
        )
    );
    let _ = writeln!(log_file, "{}", "-".repeat(64));

    let mut tail: Vec<String> = Vec::new();
    let mut errors = robocopy::ErrorLog::default();
    let mut bytes_done = 0u64;
    let mut files_done = 0u64;
    // Size of the last file line, credited once the next file line arrives
    // (or robocopy ends) with no error in between. A file that fails prints
    // its line again for every retry: counted on sight, it was counted once
    // per attempt, and as copied even when it never was.
    let mut unconfirmed: Option<u64> = None;
    let mut current_file = String::new();
    let start = Instant::now();
    let mut last_sample = 0u128;

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            break;
        }
        let next = tokio::time::timeout(std::time::Duration::from_millis(150), lines.next_line());
        match next.await {
            Ok(Ok(Some(line))) => {
                let _ = writeln!(log_file, "{line}");
                errors.push(&line);
                tail.push(line.clone());
                if tail.len() > 80 {
                    tail.remove(0);
                }
                match parser.parse(&line) {
                    robocopy::RcLine::File { bytes, path } => {
                        if let Some(done) = unconfirmed.replace(bytes) {
                            bytes_done += done;
                            files_done += 1;
                        }
                        current_file = path;
                    }
                    // Deleted (mirror) or left alone (add-only): not part of
                    // the bytes being copied. Folders are not listed (/NDL).
                    robocopy::RcLine::Extra | robocopy::RcLine::Dir { .. } => {}
                    robocopy::RcLine::Other(line) => {
                        if robocopy::is_error_line(&line) {
                            unconfirmed = None;
                        }
                    }
                }
            }
            Ok(Ok(None)) => {
                // EOF: robocopy has exited, so its last file is done.
                if let Some(done) = unconfirmed.take() {
                    bytes_done += done;
                    files_done += 1;
                }
                break;
            }
            Ok(Err(e)) => {
                let _ = writeln!(log_file, "[read error: {e}]");
            }
            Err(_) => { /* polling timeout, only so `cancel` gets checked */ }
        }

        let now_ms = start.elapsed().as_millis();
        if now_ms - last_sample >= PROGRESS_SAMPLE_INTERVAL_MS {
            last_sample = now_ms;
            emit(JobEvent::Progress(ProgressSample {
                job_id: job_id.to_string(),
                at_ms: now_ms as u64,
                bytes_done,
                bytes_total: scan.totals.bytes_to_copy.max(bytes_done),
                files_done,
                files_total: scan.totals.files_to_copy.max(files_done),
                current_file: current_file.clone(),
                current_file_bytes_done: 0,
                current_file_bytes_total: 0,
            }));
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("robocopy did not exit cleanly: {e}"))?;
    let exit_code = status.code().unwrap_or(-1);

    // Final sample: reflects the actual state rather than a forced 100%.
    emit(JobEvent::Progress(ProgressSample {
        job_id: job_id.to_string(),
        at_ms: start.elapsed().as_millis() as u64,
        bytes_done,
        bytes_total: scan.totals.bytes_to_copy.max(bytes_done),
        files_done,
        files_total: scan.totals.files_to_copy.max(files_done),
        current_file: String::new(),
        current_file_bytes_done: 0,
        current_file_bytes_total: 0,
    }));

    let _ = writeln!(log_file, "{}", "-".repeat(64));
    let _ = writeln!(
        log_file,
        "END  : {}   EXIT: {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        exit_code
    );
    let _ = log_file.flush();

    // A cancelled (killed) robocopy prints no summary: count what this run
    // saw finish, so the result and the history do not claim nothing was
    // copied. The file in hand when it was stopped is left out, so this is a
    // lower bound; the folders are not known.
    let summary = robocopy::parse_summary(&tail, exit_code).unwrap_or(RobocopySummary {
        exit_code,
        dirs_copied: 0,
        files_copied: files_done,
        files_failed: 0,
        files_extra: 0,
        bytes_copied: bytes_done,
        bytes_failed: 0,
    });
    Ok((exit_code, summary, errors.summary()))
}

/// Restic counterpart of `run_copy`: same line-reading loop with cancellation
/// polling, but progress comes from restic's structured `--json` output.
async fn run_restic_job(
    job_id: &str,
    profile: &Profile,
    restic_password: Option<String>,
    dry_run: bool,
    log_path: &Path,
    cancel: Arc<AtomicBool>,
    emit: &(impl Fn(JobEvent) + Send + Sync + 'static),
) -> JobResult {
    let start = Instant::now();
    let log_path_str = log_path.display().to_string();
    macro_rules! fail {
        ($outcome:expr, $error:expr) => {
            return JobResult {
                outcome: $outcome,
                summary: None,
                error: Some($error),
                log_path: log_path_str.clone(),
                elapsed_secs: start.elapsed().as_secs_f64(),
            }
        };
    }

    let repo = profile.destination.clone();
    let password = match restic_password {
        Some(p) if !p.is_empty() => p,
        _ => fail!(
            Outcome::Failed,
            "This job uses restic: enter the repository password.".into()
        ),
    };

    let phase = |phase: JobPhase, detail: String| {
        emit(JobEvent::Phase {
            job_id: job_id.to_string(),
            phase,
            detail: Some(detail),
        })
    };

    phase(
        JobPhase::Connecting,
        format!("opening the restic repository at {repo}"),
    );
    match restic::is_initialized(&repo, &password).await {
        Ok(true) => {}
        Ok(false) => {
            phase(
                JobPhase::Connecting,
                format!("no repository there yet: creating one at {repo}"),
            );
            if let Err(e) = restic::init_repo(&repo, &password).await {
                fail!(
                    Outcome::Failed,
                    format!("could not initialise the restic repository: {e}")
                );
            }
        }
        Err(e @ restic::ResticError::WrongPassword) => fail!(Outcome::Failed, e.to_string()),
        Err(e) => fail!(Outcome::ConnectionError, e.to_string()),
    }

    phase(
        JobPhase::Copying,
        format!(
            "restic backup \"{}\" --json{}",
            profile.source,
            if dry_run { " --dry-run" } else { "" }
        ),
    );

    let mut child = match restic::start_backup(
        &profile.source,
        &repo,
        &password,
        &profile.advanced,
        dry_run,
    ) {
        Ok(c) => c,
        Err(e) => fail!(Outcome::Failed, e.to_string()),
    };

    let Some(stdout) = child.stdout.take() else {
        fail!(Outcome::Failed, "restic produced no output".into());
    };
    let mut lines = BufReader::new(stdout).lines();
    // Drain stderr concurrently and keep the last lines: that is where restic
    // explains failures (wrong password, locked repository, ...).
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut tail: Vec<String> = Vec::new();
            let mut err_lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = err_lines.next_line().await {
                tail.push(line);
                if tail.len() > 50 {
                    tail.remove(0);
                }
            }
            tail
        })
    });
    let mut log_file = match std::fs::File::create(log_path).map(std::io::BufWriter::new) {
        Ok(f) => f,
        Err(e) => fail!(Outcome::Failed, format!("could not create the log file: {e}")),
    };
    let _ = writeln!(
        log_file,
        "START : {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    let _ = writeln!(log_file, "ENGINE: restic");
    let _ = writeln!(log_file, "SRC   : {}", profile.source);
    let _ = writeln!(log_file, "REPO  : {repo}");
    let _ = writeln!(log_file, "{}", "-".repeat(64));

    let mut last_sample = 0u128;
    let mut summary: Option<ResticSummary> = None;

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            break;
        }
        let next = tokio::time::timeout(std::time::Duration::from_millis(150), lines.next_line());
        match next.await {
            Ok(Ok(Some(line))) => {
                let _ = writeln!(log_file, "{line}");
                match restic::parse_line(&line) {
                    restic::Event::Status(s) => {
                        let now_ms = start.elapsed().as_millis();
                        if now_ms - last_sample >= PROGRESS_SAMPLE_INTERVAL_MS {
                            last_sample = now_ms;
                            emit(JobEvent::Progress(ProgressSample {
                                job_id: job_id.to_string(),
                                at_ms: now_ms as u64,
                                bytes_done: s.bytes_done,
                                bytes_total: s.bytes_total,
                                files_done: s.files_done,
                                files_total: s.files_total,
                                current_file: s.current_file,
                                current_file_bytes_done: 0,
                                current_file_bytes_total: 0,
                            }));
                        }
                    }
                    restic::Event::Summary(s) => summary = Some(s),
                    restic::Event::Other => {}
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                let _ = writeln!(log_file, "[read error: {e}]");
            }
            Err(_) => {}
        }
    }

    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => fail!(Outcome::Failed, format!("restic did not exit cleanly: {e}")),
    };
    let stderr_tail = match stderr_task {
        Some(task) => task.await.unwrap_or_default(),
        None => Vec::new(),
    };
    for line in &stderr_tail {
        let _ = writeln!(log_file, "[stderr] {line}");
    }
    let _ = writeln!(log_file, "{}", "-".repeat(64));
    let _ = writeln!(
        log_file,
        "END   : {}   EXIT: {:?}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        status.code()
    );
    let _ = log_file.flush();

    let summary_out = summary.clone().map(EngineSummary::Restic);

    if cancel.load(Ordering::SeqCst) {
        return JobResult {
            outcome: Outcome::Cancelled,
            summary: summary_out,
            error: None,
            log_path: log_path_str,
            elapsed_secs: start.elapsed().as_secs_f64(),
        };
    }
    let last_stderr = stderr_tail.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty());
    let exit = restic::classify_backup_exit(status.code());
    if exit == restic::BackupExit::Failed {
        return JobResult {
            outcome: Outcome::Failed,
            summary: summary_out,
            error: Some(match last_stderr {
                Some(last) => format!("restic exited with code {:?}: {last}", status.code()),
                None => format!("restic exited with code {:?}", status.code()),
            }),
            log_path: log_path_str,
            elapsed_secs: start.elapsed().as_secs_f64(),
        };
    }
    let incomplete = exit == restic::BackupExit::Incomplete;
    let (outcome, warning) = if incomplete {
        let warning = match last_stderr {
            Some(last) => format!("some files could not be read, so the snapshot is incomplete: {last}"),
            None => "some files could not be read, so the snapshot is incomplete (see the log)".into(),
        };
        (Outcome::SuccessWithMismatches, Some(warning))
    } else {
        let outcome = match &summary {
            Some(s) if s.files_new == 0 && s.files_changed == 0 => Outcome::NoChanges,
            _ => Outcome::Success,
        };
        (outcome, None)
    };

    if incomplete {
        // Retention keeps the newest snapshot of each day, week and month, so
        // an incomplete one could push out a complete one: wait for a
        // complete run.
        let _ = writeln!(log_file, "[forget --prune skipped: the snapshot is incomplete]");
    } else if !dry_run {
        if let Err(e) = restic::forget_and_prune(
            &repo,
            &password,
            profile.restic.keep_daily,
            profile.restic.keep_weekly,
            profile.restic.keep_monthly,
        )
        .await
        {
            let _ = writeln!(log_file, "[forget --prune failed: {e}]");
        }
    }
    let _ = log_file.flush();

    JobResult {
        outcome,
        summary: summary_out,
        error: warning,
        log_path: log_path_str,
        elapsed_secs: start.elapsed().as_secs_f64(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize("  C:/Data/Photos/ "), "c:\\data\\photos");
        assert_eq!(normalize(r"\\NAS\Backup\"), r"\\nas\backup");
    }

    #[test]
    fn share_root_extracts_host_and_share() {
        assert_eq!(
            share_root(r"\\nas\backup\photos").as_deref(),
            Some(r"\\nas\backup")
        );
        assert_eq!(share_root(r"\\nas\backup").as_deref(), Some(r"\\nas\backup"));
        assert_eq!(share_root(r"\\nas"), None);
        assert_eq!(share_root(r"D:\backups"), None);
    }

    #[test]
    fn share_root_accepts_every_network_spelling() {
        // Whatever `is_network_path` accepts must yield a share, or a job with
        // credentials would skip the connection.
        for path in ["//nas/backup/photos", r"  \\nas\backup\photos ", r"\\nas/backup"] {
            assert!(is_network_path(path), "{path:?}");
            assert_eq!(share_root(path).as_deref(), Some(r"\\nas\backup"), "{path:?}");
        }
        assert_eq!(share_root("C:/data"), None);
    }

    #[tokio::test]
    async fn paths_are_trimmed_before_validating() {
        let base = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        let src = base.join("source");
        std::fs::create_dir_all(&src).unwrap();
        let profile = Profile {
            id: "p1".into(),
            name: "Spaces".into(),
            source: format!("  {}  ", src.display()),
            destination: format!(" {} ", base.join("dest").display()),
            mode: types::CopyMode::Accumulate,
            credentials: None,
            wake_on_lan: None,
            advanced: Default::default(),
            engine: Engine::Robocopy,
            restic: Default::default(),
        };
        let request = JobRequest {
            profile,
            password: None,
            restic_password: None,
            dry_run: true,
            preview: None,
        };
        let logs = base.join("logs");
        let result = run_job(&JobContext::new("j1".into()), request, &logs, |_| {}).await;
        // Whatever robocopy then does, the source must have been found.
        let error = result.error.unwrap_or_default();
        assert!(!error.contains("does not exist"), "{error}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn validate_paths_rules() {
        let src = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&src).unwrap();
        let src_s = src.display().to_string();

        assert!(matches!(
            validate_paths(&format!("{src_s}-missing"), r"D:\x"),
            Err(PathError::SourceMissing(_))
        ));
        assert!(matches!(
            validate_paths(&src_s, "  "),
            Err(PathError::DestinationEmpty)
        ));
        assert!(matches!(
            validate_paths(&src_s, &format!("{}\\", src_s.to_uppercase())),
            Err(PathError::SameSourceAndDestination)
        ));
        assert!(matches!(
            validate_paths(&src_s, &format!("{src_s}\\sub")),
            Err(PathError::DestinationInsideSource)
        ));
        // A sibling sharing the same prefix is NOT inside the source.
        assert!(validate_paths(&src_s, &format!("{src_s}-copy")).is_ok());
        // The source's parent as destination: /MIR would delete the source.
        let parent_s = src.parent().unwrap().display().to_string();
        assert!(matches!(
            validate_paths(&src_s, &parent_s),
            Err(PathError::SourceInsideDestination)
        ));

        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn validate_paths_rejects_paths_that_are_not_full() {
        let src = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&src).unwrap();
        let src_s = src.display().to_string();

        // `\nas\share` (one backslash short) would land in C:\nas\share.
        for dst in [r"\nas\share\copy", "backup", r"D:backup", "D:", r"\\nas"] {
            assert!(
                matches!(validate_paths(&src_s, dst), Err(PathError::NotAbsolute(_))),
                "{dst:?}"
            );
        }
        assert!(matches!(
            validate_paths(r"\COSAS", r"D:\mirror"),
            Err(PathError::NotAbsolute(_))
        ));
        for dst in [r"D:\", "D:/mirror", r"\\nas\share", "//nas/share/copy"] {
            assert!(validate_paths(&src_s, dst).is_ok(), "{dst:?}");
        }

        let _ = std::fs::remove_dir_all(&src);
    }

    /// A file robocopy cannot read fails the run (exit code 8), and the
    /// result has to say which file and why, not just "failed".
    #[cfg(windows)]
    #[tokio::test]
    async fn a_failed_copy_says_why() {
        use std::os::windows::fs::OpenOptionsExt;

        let base = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        let src = base.join("source");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("ok.txt"), "ok").unwrap();
        std::fs::write(src.join("locked.txt"), "locked").unwrap();
        // No sharing at all: robocopy cannot open it.
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(src.join("locked.txt"))
            .unwrap();
        let profile = Profile {
            id: "p1".into(),
            name: "Locked".into(),
            source: src.display().to_string(),
            destination: base.join("dest").display().to_string(),
            mode: types::CopyMode::Mirror,
            credentials: None,
            wake_on_lan: None,
            advanced: types::AdvancedOptions {
                // One retry: the locked file is attempted, and listed, twice.
                retries: 1,
                wait_secs: 0,
                ..Default::default()
            },
            engine: Engine::Robocopy,
            restic: Default::default(),
        };
        let request = JobRequest {
            profile,
            password: None,
            restic_password: None,
            dry_run: false,
            preview: None,
        };
        let (result, events) = run_with_events(request, &base.join("logs")).await;
        assert_eq!(result.outcome, Outcome::Failed);
        let error = result.error.unwrap_or_default();
        assert!(error.contains("locked.txt"), "{error}");
        match result.summary {
            Some(EngineSummary::Robocopy(s)) => assert_eq!((s.files_copied, s.files_failed), (1, 1)),
            other => panic!("expected a robocopy summary, got {other:?}"),
        }
        // The progress counts ok.txt only: not the failed file, nor its retry.
        let last = events.iter().rev().find_map(|e| match e {
            JobEvent::Progress(p) => Some(p.clone()),
            _ => None,
        });
        let last = last.expect("a progress sample");
        assert_eq!((last.files_done, last.bytes_done), (1, 2));

        drop(_lock);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn validate_paths_does_not_touch_network_sources() {
        // Not checked for existence (the share may be asleep or need the
        // job's login), but still checked for nesting.
        assert!(validate_paths(r"\\nas\share\photos", r"D:\mirror").is_ok());
        assert!(matches!(
            validate_paths(r"\\nas\share", r"\\NAS\share\sub"),
            Err(PathError::DestinationInsideSource)
        ));
        assert!(matches!(
            validate_paths(r"\\nas\share\photos", r"\\nas\share"),
            Err(PathError::SourceInsideDestination)
        ));
    }

    #[tokio::test]
    async fn rejected_run_leaves_a_history_entry() {
        let log_dir = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        let missing = log_dir.join("no-such-source").display().to_string();
        let profile = Profile {
            id: "p1".into(),
            name: "Unplugged drive".into(),
            source: missing,
            destination: r"D:\mirror".into(),
            mode: types::CopyMode::Mirror,
            credentials: None,
            wake_on_lan: None,
            advanced: Default::default(),
            engine: Engine::Robocopy,
            restic: Default::default(),
        };
        let request = JobRequest {
            profile,
            password: None,
            restic_password: None,
            dry_run: false,
            preview: None,
        };
        let result = run_job(&JobContext::new("j1".into()), request, &log_dir, |_| {}).await;
        assert_eq!(result.outcome, Outcome::Failed);

        let entries = crate::history::list(&log_dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].profile_name, "Unplugged drive");
        assert_eq!(entries[0].outcome, Some(Outcome::Failed));
        assert!(entries[0].error.as_deref().unwrap_or("").contains("does not exist"));
        assert!(!entries[0].has_log);

        let _ = std::fs::remove_dir_all(&log_dir);
    }

    #[test]
    fn lexical_normalize_resolves_dot_dot() {
        assert_eq!(lexical_normalize(r"C:\a\b\..\c"), r"C:\a\c");
        assert_eq!(lexical_normalize(r"C:\a\.\b"), r"C:\a\b");
        // An extra `..` does not escape the root.
        assert_eq!(lexical_normalize(r"C:\..\..\a"), r"C:\a");
        assert_eq!(lexical_normalize(r"\\nas\backup\x\..\y"), r"\\nas\backup\y");
    }

    #[test]
    fn validate_paths_resolves_traversal() {
        let base = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        let src = base.join("source");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        let src_s = src.display().to_string();

        // `source\sub\..` is the source itself, even though the text differs.
        assert!(matches!(
            validate_paths(&src_s, &format!(r"{src_s}\sub\..")),
            Err(PathError::SameSourceAndDestination)
        ));
        // `source\..\source\data` is still inside the source.
        assert!(matches!(
            validate_paths(&src_s, &format!(r"{src_s}\..\source\data")),
            Err(PathError::DestinationInsideSource)
        ));
        // Actually leaving the source is fine, even if the destination does not exist yet.
        assert!(validate_paths(&src_s, &format!(r"{src_s}\..\dest")).is_ok());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn robocopy_exit_codes() {
        assert_eq!(classify_exit_code(0), Outcome::Success);
        assert_eq!(classify_exit_code(1), Outcome::Success);
        assert_eq!(classify_exit_code(3), Outcome::Success);
        assert_eq!(classify_exit_code(4), Outcome::SuccessWithMismatches);
        assert_eq!(classify_exit_code(9), Outcome::Failed);
        assert_eq!(classify_exit_code(16), Outcome::Failed);
    }

    /// Runs `run_job` and keeps the events it sends.
    async fn run_with_events(request: JobRequest, log_dir: &Path) -> (JobResult, Vec<JobEvent>) {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let result = run_job(&JobContext::new("j1".into()), request, log_dir, move |e| {
            sink.lock().unwrap().push(e)
        })
        .await;
        let events = events.lock().unwrap().clone();
        (result, events)
    }

    /// Confirming a preview copies without scanning again, and the progress
    /// is measured against the preview's totals.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_confirmed_preview_copies_without_scanning_again() {
        let base = std::env::temp_dir().join(format!("nasmirror-test-{}", uuid::Uuid::new_v4()));
        let src = base.join("source");
        std::fs::create_dir_all(src.join("empty")).unwrap();
        std::fs::write(src.join("a.txt"), "1234").unwrap();
        let profile = Profile {
            id: "p1".into(),
            name: "Preview".into(),
            source: src.display().to_string(),
            destination: base.join("dest").display().to_string(),
            mode: types::CopyMode::Mirror,
            credentials: None,
            wake_on_lan: None,
            advanced: Default::default(),
            engine: Engine::Robocopy,
            restic: Default::default(),
        };
        let preview = types::ScanTotals {
            files_to_copy: 1,
            bytes_to_copy: 4,
            files_to_delete: 0,
            dirs_to_create: 2,
        };
        let request = JobRequest {
            profile,
            password: None,
            restic_password: None,
            dry_run: false,
            preview: Some(preview),
        };
        let (result, events) = run_with_events(request, &base.join("logs")).await;

        assert_eq!(result.outcome, Outcome::Success, "{:?}", result.error);
        assert!(base.join("dest").join("empty").is_dir());
        assert!(!events.iter().any(|e| matches!(
            e,
            JobEvent::Phase { phase: JobPhase::Scanning, .. }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            JobEvent::ScanResult { totals, warning: None, .. }
                if totals.files_to_copy == 1 && totals.dirs_to_create == 2
        )));
        let last = events.iter().rev().find_map(|e| match e {
            JobEvent::Progress(p) => Some(p.clone()),
            _ => None,
        });
        let last = last.expect("a progress sample");
        assert_eq!((last.files_done, last.bytes_done, last.bytes_total), (1, 4, 4));

        let _ = std::fs::remove_dir_all(&base);
    }
}
