use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::process::{Child, Command};

use super::console::ConsoleLines;
use super::types::{AdvancedOptions, CopyMode, RobocopySummary};

/// Flags shared by the scan (`/L`) and the actual copy. Deliberately does NOT
/// include `/XX`: in mirror mode robocopy must report (and count) the files it
/// will delete from the destination, otherwise a job whose only work is
/// deletions would be reported as "no changes" and never run.
pub fn common_args(mode: CopyMode, adv: &AdvancedOptions) -> Vec<String> {
    let mut args = vec![
        "/BYTES".into(),
        "/NP".into(),
        "/NC".into(),
        "/NDL".into(),
        format!("/R:{}", adv.retries),
        format!("/W:{}", adv.wait_secs),
    ];
    if adv.fat_time_tolerance {
        args.push("/FFT".into());
    }
    if adv.dst_adjust {
        args.push("/DST".into());
    }
    for d in &adv.exclude_dirs {
        if !d.is_empty() {
            args.push("/XD".into());
            args.push(d.clone());
        }
    }
    for f in &adv.exclude_files {
        if !f.is_empty() {
            args.push("/XF".into());
            args.push(f.clone());
        }
    }
    for x in &adv.extra_flags {
        if !x.is_empty() {
            args.push(x.clone());
        }
    }
    let _ = mode; // the mode flag (/E or /MIR) is added separately, next to src/dst
    args
}

pub fn copy_only_args(adv: &AdvancedOptions) -> Vec<String> {
    let mut args = vec![];
    if adv.threads > 0 {
        args.push(format!("/MT:{}", adv.threads));
    }
    if adv.unbuffered_io {
        args.push("/J".into());
    }
    args
}

/// A robocopy file line with `/BYTES /NP /NC /NDL`: tab, size in bytes, tab,
/// file path.
static FILE_LINE_RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
fn file_line_re() -> &'static regex_lite::Regex {
    FILE_LINE_RE.get_or_init(|| regex_lite::Regex::new(r"^\s*(\d+)\t(.+)$").unwrap())
}

/// Mirror-mode deletion line: `*EXTRA File` / `*EXTRA Dir` followed by size and path.
static EXTRA_LINE_RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
fn extra_line_re() -> &'static regex_lite::Regex {
    EXTRA_LINE_RE.get_or_init(|| regex_lite::Regex::new(r"^\*EXTRA\s+\S+").unwrap())
}

pub enum RcLine {
    File { bytes: u64, path: String },
    ExtraDeleted,
    Other(String),
}

pub fn parse_line(line: &str) -> RcLine {
    if let Some(caps) = file_line_re().captures(line) {
        let bytes: u64 = caps[1].parse().unwrap_or(0);
        return RcLine::File {
            bytes,
            path: caps[2].trim().to_string(),
        };
    }
    if extra_line_re().is_match(line.trim_start()) {
        return RcLine::ExtraDeleted;
    }
    RcLine::Other(line.to_string())
}

fn spawn_robocopy(src: &str, dst: &str, mode_flag: &str, args: &[String]) -> std::io::Result<Child> {
    let mut cmd = Command::new("robocopy");
    cmd.arg(src).arg(dst).arg(mode_flag).args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.stdin(Stdio::null());
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
}

pub struct ScanOutcome {
    pub files_to_copy: u64,
    pub bytes_to_copy: u64,
    pub files_to_delete: u64,
}

/// Scans with `/L` (list-only, changes nothing) to know in advance what the
/// copy will do, so an accurate preview can be shown before confirming.
///
/// On large trees or slow destinations the scan can take minutes, so it
/// honours `cancel`: returns `Ok(None)` if the user stopped it.
pub async fn scan(
    src: &str,
    dst: &str,
    mode: CopyMode,
    adv: &AdvancedOptions,
    cancel: Arc<AtomicBool>,
) -> std::io::Result<Option<ScanOutcome>> {
    let mut args = common_args(mode, adv);
    args.push("/L".into());
    let mut child = spawn_robocopy(src, dst, mode.robocopy_flag(), &args)?;
    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = ConsoleLines::new(stdout);

    let mut files_to_copy = 0u64;
    let mut bytes_to_copy = 0u64;
    let mut files_to_delete = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(None);
        }
        // Short timeout so `cancel` is checked even while robocopy is slow
        // to write the next line.
        let next = tokio::time::timeout(Duration::from_millis(150), reader.next_line());
        let line = match next.await {
            Ok(line) => match line? {
                Some(line) => line,
                None => break,
            },
            Err(_) => continue,
        };
        match parse_line(&line) {
            RcLine::File { bytes, .. } => {
                files_to_copy += 1;
                bytes_to_copy += bytes;
            }
            RcLine::ExtraDeleted => files_to_delete += 1,
            RcLine::Other(_) => {}
        }
    }
    // A code >= 8 means robocopy could not read the source or destination.
    // Without this check an empty scan would be taken as "no changes" and a
    // scheduled task would report success without copying anything.
    let status = child.wait().await?;
    let code = status.code().unwrap_or(-1);
    if !(0..8).contains(&code) {
        return Err(std::io::Error::other(format!(
            "robocopy exited with code {code} while scanning"
        )));
    }
    Ok(Some(ScanOutcome {
        files_to_copy,
        bytes_to_copy,
        files_to_delete,
    }))
}

pub fn start_copy(
    src: &str,
    dst: &str,
    mode: CopyMode,
    adv: &AdvancedOptions,
) -> std::io::Result<Child> {
    let mut args = common_args(mode, adv);
    args.extend(copy_only_args(adv));
    spawn_robocopy(src, dst, mode.robocopy_flag(), &args)
}

/// In every Windows language, robocopy prints a final block of rows
/// "<Label> :  Total  Copied  Skipped  Mismatch  FAILED  Extras". The label
/// text is localized, so data rows are identified by position instead
/// (1st = Dirs, 2nd = Files, 3rd = Bytes), which is stable across locales.
pub fn parse_summary(tail_lines: &[String], exit_code: i32) -> Option<RobocopySummary> {
    let mut rows: Vec<[String; 6]> = vec![];
    for line in tail_lines {
        let Some(idx) = line.find(':') else { continue };
        let rest = line[idx + 1..].trim();
        let toks: Vec<&str> = rest.split_whitespace().collect();
        if toks.len() == 6
            && toks
                .iter()
                .all(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        {
            rows.push([
                toks[0].to_string(),
                toks[1].to_string(),
                toks[2].to_string(),
                toks[3].to_string(),
                toks[4].to_string(),
                toks[5].to_string(),
            ]);
        }
        if rows.len() == 3 {
            break;
        }
    }
    if rows.len() < 3 {
        return None;
    }
    let dirs = &rows[0];
    let files = &rows[1];
    let bytes = &rows[2];
    Some(RobocopySummary {
        exit_code,
        dirs_copied: parse_count(&dirs[1]),
        files_copied: parse_count(&files[1]),
        files_failed: parse_count(&files[4]),
        files_extra: parse_count(&files[5]),
        bytes_copied: parse_size(&bytes[1]),
        bytes_failed: parse_size(&bytes[4]),
    })
}

fn parse_count(tok: &str) -> u64 {
    tok.parse().unwrap_or(0)
}

/// Robocopy abbreviates large byte counts with a one-letter suffix (k/m/g/t,
/// base 1024) attached to the number, e.g. "45.2m".
fn parse_size(tok: &str) -> u64 {
    let tok = tok.trim();
    if tok.is_empty() {
        return 0;
    }
    let last = tok.chars().last().unwrap();
    let (num_part, mult): (&str, f64) = if last.is_ascii_alphabetic() {
        let mult = match last.to_ascii_lowercase() {
            'k' => 1024.0,
            'm' => 1024.0 * 1024.0,
            'g' => 1024.0 * 1024.0 * 1024.0,
            't' => 1024.0 * 1024.0 * 1024.0 * 1024.0,
            _ => 1.0,
        };
        (&tok[..tok.len() - last.len_utf8()], mult)
    } else {
        (tok, 1.0)
    };
    num_part
        .trim()
        .parse::<f64>()
        .map(|v| (v * mult).round() as u64)
        .unwrap_or(0)
}

/// Log file path for this run. Adds a numeric suffix if that second is already
/// taken: two back-to-back jobs (e.g. a scheduled task, or one that fails
/// immediately) can start within the same second, and the second job would
/// otherwise overwrite the first one's log and history record.
pub fn default_log_path(log_dir: &Path) -> std::path::PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let taken = |p: &Path| p.exists() || p.with_extension("json").exists();
    let first = log_dir.join(format!("backup_{stamp}.log"));
    if !taken(&first) {
        return first;
    }
    for n in 2..1000 {
        let candidate = log_dir.join(format!("backup_{stamp}_{n}.log"));
        if !taken(&candidate) {
            return candidate;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_paths_do_not_collide_within_the_same_second() {
        let dir = std::env::temp_dir().join(format!("nasmirror-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let first = default_log_path(&dir);
        std::fs::write(&first, "").unwrap();
        let second = default_log_path(&dir);
        assert_ne!(first, second);

        // A run that fails before copying leaves only the .json record:
        // that second counts as taken too.
        std::fs::write(second.with_extension("json"), "{}").unwrap();
        let third = default_log_path(&dir);
        assert_ne!(third, first);
        assert_ne!(third, second);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_line_file() {
        match parse_line("\t\t        12345\tC:\\data\\photo.jpg") {
            RcLine::File { bytes, path } => {
                assert_eq!(bytes, 12345);
                assert_eq!(path, "C:\\data\\photo.jpg");
            }
            _ => panic!("expected a file line"),
        }
    }

    #[test]
    fn parse_line_extra_and_other() {
        assert!(matches!(
            parse_line("\t\t*EXTRA File \t\t  100\told.txt"),
            RcLine::ExtraDeleted
        ));
        // Localized (Spanish) robocopy header line.
        assert!(matches!(
            parse_line("   Origen : C:\\data\\"),
            RcLine::Other(_)
        ));
    }

    #[test]
    fn parse_summary_rows_by_position() {
        // Real Spanish-locale robocopy summary: labels are localized, so
        // rows must be matched by position.
        let tail: Vec<String> = [
            "               Total    Copiado   Omitido  No coinc.    ERROR    Extras",
            "Directorios :         5         2         3         0         0         0",
            "   Archivos :        10         4         6         0         1         2",
            "      Bytes :      2048      1024      1024         0       512         0",
            "     Tiempos :   0:00:05   0:00:02                       0:00:00   0:00:03",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let s = parse_summary(&tail, 9).expect("summary");
        assert_eq!(s.exit_code, 9);
        assert_eq!(s.dirs_copied, 2);
        assert_eq!(s.files_copied, 4);
        assert_eq!(s.files_failed, 1);
        assert_eq!(s.files_extra, 2);
        assert_eq!(s.bytes_copied, 1024);
        assert_eq!(s.bytes_failed, 512);
    }

    #[test]
    fn parse_summary_incomplete() {
        assert!(parse_summary(&["nothing to see here".to_string()], 0).is_none());
    }

    #[test]
    fn parse_size_suffixes() {
        assert_eq!(parse_size("0"), 0);
        assert_eq!(parse_size("1500"), 1500);
        assert_eq!(parse_size("2k"), 2048);
        assert_eq!(parse_size("1.5m"), 1_572_864);
        assert_eq!(parse_size("1g"), 1024 * 1024 * 1024);
        assert_eq!(parse_size(""), 0);
        assert_eq!(parse_size("abc"), 0);
    }

    #[test]
    fn args_include_options() {
        let adv = AdvancedOptions {
            fat_time_tolerance: true,
            exclude_dirs: vec![".git".into(), String::new()],
            exclude_files: vec!["*.tmp".into()],
            ..AdvancedOptions::default()
        };
        let args = common_args(CopyMode::Mirror, &adv);
        assert!(args.contains(&"/FFT".to_string()));
        assert!(args.windows(2).any(|w| w[0] == "/XD" && w[1] == ".git"));
        assert!(args.windows(2).any(|w| w[0] == "/XF" && w[1] == "*.tmp"));
        assert_eq!(args.iter().filter(|a| *a == "/XD").count(), 1);
        assert!(!args.contains(&"/L".to_string()));

        assert_eq!(copy_only_args(&adv), vec!["/MT:16".to_string()]);
    }
}
