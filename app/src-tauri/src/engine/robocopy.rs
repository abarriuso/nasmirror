use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::process::{Child, Command};

use super::console::ConsoleLines;
use super::types::{AdvancedOptions, CopyMode, RobocopySummary, ScanTotals};

/// Flags shared by the scan (`/L`) and the actual copy. Deliberately does NOT
/// include `/XX`: in mirror mode robocopy must report (and count) the files it
/// will delete from the destination, otherwise a job whose only work is
/// deletions would be reported as "no changes" and never run.
///
/// `/NC` drops the file class ("New File", "*EXTRA File"), which is localized;
/// `/FP` keeps full paths, which is how `LineParser` tells the destination's
/// extra files apart from the files being copied. Folders are listed in the
/// scan, so the window can show where it is; the copy leaves them out
/// (`/NDL`, see `copy_only_args`).
pub fn common_args(mode: CopyMode, adv: &AdvancedOptions) -> Vec<String> {
    let mut args = vec![
        "/BYTES".into(),
        "/NP".into(),
        "/NC".into(),
        "/FP".into(),
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

/// The copy does not list folders: one line per folder would fill the log
/// of a large tree with folders it did nothing in.
pub fn copy_only_args(adv: &AdvancedOptions) -> Vec<String> {
    let mut args = vec!["/NDL".to_string()];
    if adv.threads > 0 {
        args.push(format!("/MT:{}", adv.threads));
    }
    if adv.unbuffered_io {
        args.push("/J".into());
    }
    args
}

/// A robocopy file or folder line with `/BYTES /NP /NC /FP`: tab, size in
/// bytes (for a folder, its number of files), tab, full path; a folder's ends
/// with a backslash. An extra folder in the destination has size -1.
static FILE_LINE_RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
fn file_line_re() -> &'static regex_lite::Regex {
    FILE_LINE_RE.get_or_init(|| regex_lite::Regex::new(r"^\s*(-?\d+)\t(.+)$").unwrap())
}

pub enum RcLine {
    /// A file robocopy copies (or, in a scan, would copy).
    File {
        bytes: u64,
        path: String,
    },
    /// A file or folder that only exists in the destination. Mirror mode
    /// deletes it; add-only mode just lists it.
    Extra,
    /// A source folder robocopy has reached (only the scan lists them).
    Dir { path: String },
    Other(String),
}

/// Classifies robocopy's output lines for one job.
///
/// With `/NC` a destination-only file ("*EXTRA File") is printed exactly like
/// a file to copy, so the path is what tells them apart: robocopy prints it
/// under the destination (resolved like `GetFullPathName`: `/` turned into
/// `\`, `..` resolved, the case kept as typed), while files to copy come from
/// the source. `validate_paths` guarantees that neither folder contains the
/// other.
pub struct LineParser {
    /// Lower-case destination with a trailing `\`.
    dst_prefix: String,
}

impl LineParser {
    pub fn new(dst: &str) -> Self {
        Self {
            dst_prefix: format!("{}\\", super::normalize(&super::lexical_normalize(dst))),
        }
    }

    pub fn parse(&self, line: &str) -> RcLine {
        let Some(caps) = file_line_re().captures(line) else {
            return RcLine::Other(line.to_string());
        };
        let path = caps[2].trim();
        if path.to_lowercase().starts_with(&self.dst_prefix) {
            return RcLine::Extra;
        }
        if path.ends_with('\\') {
            return RcLine::Dir {
                path: path.to_string(),
            };
        }
        match caps[1].parse::<u64>() {
            Ok(bytes) => RcLine::File {
                bytes,
                path: path.to_string(),
            },
            Err(_) => RcLine::Other(line.to_string()),
        }
    }
}

/// `<date> <time> ERROR <code> (0x<8 hex digits>) <what it was doing> <path>`.
/// The word is translated in some languages (FEHLER, ERREUR), so the line is
/// recognised by its shape and the word is captured.
static ERROR_LINE_RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
fn error_line_re() -> &'static regex_lite::Regex {
    ERROR_LINE_RE.get_or_init(|| {
        regex_lite::Regex::new(r"(\S+) \d+ \(0x[0-9A-Fa-f]{8}\) (.+)$").unwrap()
    })
}

/// Whether robocopy is reporting a failed attempt on this line.
pub fn is_error_line(line: &str) -> bool {
    error_line_re().is_match(line)
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The distinct errors of one kind: the first one in full, the rest counted.
/// Hashes keep it cheap when thousands of files fail.
#[derive(Default)]
struct DistinctErrors {
    seen: std::collections::HashSet<u64>,
    first: Option<String>,
}

impl DistinctErrors {
    fn add(&mut self, error: &str) {
        if self.seen.insert(hash_of(error)) && self.first.is_none() {
            self.first = Some(error.to_string());
        }
    }

    fn summary(self) -> Option<String> {
        let first = self.first?;
        Some(match self.seen.len() {
            1 => first,
            n => format!("{first} (and {} more errors, see the log)", n - 1),
        })
    }
}

/// Collects the errors robocopy prints while it works, so a failed run can
/// say why instead of just "failed": each one is an error line followed by
/// the system's explanation ("Access is denied."). Robocopy retries, so an
/// error is not always a failure: the ones that failed the run are followed
/// by "ERROR: RETRY LIMIT EXCEEDED." (in the same word as the error line),
/// and those are reported first.
#[derive(Default)]
pub struct ErrorLog {
    /// How this robocopy spells ERROR, from its first error line.
    word: Option<String>,
    /// An error line still waiting for its explanation on the next line.
    pending: Option<String>,
    /// The last complete error, which a retry-limit line marks as final.
    last: Option<String>,
    all: DistinctErrors,
    gave_up: DistinctErrors,
}

impl ErrorLog {
    pub fn push(&mut self, line: &str) {
        let line = line.trim();
        if self.is_retry_limit(line) {
            if let Some(what) = self.pending.take() {
                self.add(what);
            }
            if let Some(last) = self.last.take() {
                self.gave_up.add(&last);
            }
            return;
        }
        if let Some(what) = self.pending.take() {
            // With /MT the explanation can be missing, or another thread's
            // line can come in between: never take a file or error line for it.
            if line.is_empty() || is_error_line(line) || file_line_re().is_match(line) {
                self.add(what);
            } else {
                self.add(format!("{what}: {line}"));
                return;
            }
        }
        if let Some(caps) = error_line_re().captures(line) {
            self.word.get_or_insert_with(|| caps[1].to_string());
            self.pending = Some(caps[2].trim().to_string());
        }
    }

    /// "ERROR: RETRY LIMIT EXCEEDED." or its translation: the error word, a
    /// colon (French puts a space before it), and no error code.
    fn is_retry_limit(&self, line: &str) -> bool {
        let Some(word) = &self.word else {
            return false;
        };
        line.strip_prefix(word.as_str())
            .is_some_and(|rest| rest.trim_start().starts_with(':'))
            && !is_error_line(line)
    }

    fn add(&mut self, error: String) {
        self.all.add(&error);
        self.last = Some(error);
    }

    /// The first error that failed the run, and how many others there were.
    /// Without any (robocopy stopped before giving up on a file), the first
    /// error of any kind.
    pub fn summary(mut self) -> Option<String> {
        if let Some(what) = self.pending.take() {
            self.add(what);
        }
        if self.gave_up.first.is_some() {
            self.gave_up.summary()
        } else {
            self.all.summary()
        }
    }
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
    super::children::spawn(&mut cmd)
}

/// The command as it would be typed in a console, for the window's details
/// and the top of the log.
fn command_line(src: &str, dst: &str, mode_flag: &str, args: &[String]) -> String {
    let quote = |a: &str| {
        if a.is_empty() || a.contains(' ') {
            format!("\"{a}\"")
        } else {
            a.to_string()
        }
    };
    [src, dst, mode_flag]
        .into_iter()
        .chain(args.iter().map(String::as_str))
        .fold("robocopy".to_string(), |line, arg| line + " " + &quote(arg))
}

fn scan_args(mode: CopyMode, adv: &AdvancedOptions) -> Vec<String> {
    let mut args = common_args(mode, adv);
    args.push("/L".into());
    args
}

fn copy_args(mode: CopyMode, adv: &AdvancedOptions) -> Vec<String> {
    let mut args = common_args(mode, adv);
    args.extend(copy_only_args(adv));
    args
}

pub fn scan_command_line(src: &str, dst: &str, mode: CopyMode, adv: &AdvancedOptions) -> String {
    command_line(src, dst, mode.robocopy_flag(), &scan_args(mode, adv))
}

pub fn copy_command_line(src: &str, dst: &str, mode: CopyMode, adv: &AdvancedOptions) -> String {
    command_line(src, dst, mode.robocopy_flag(), &copy_args(mode, adv))
}

/// How often the scan reports how far it has got.
const SCAN_REPORT_INTERVAL: Duration = Duration::from_millis(250);

pub struct ScanOutcome {
    pub totals: ScanTotals,
    /// Robocopy reported a problem while scanning: part of the tree it could
    /// not read (exit code 8), or a destination it could not reach. What it
    /// could read can still be copied.
    pub warning: Option<String>,
}

/// Scans with `/L` (list-only, changes nothing) to know in advance what the
/// copy will do, so an accurate preview can be shown before confirming.
///
/// On large trees or slow destinations the scan can take minutes, so it
/// honours `cancel` (returns `Ok(None)` if the user stopped it), and calls
/// `report` a few times a second with the folders it has gone through, the
/// one it is in, and what it has found to do so far.
pub async fn scan(
    src: &str,
    dst: &str,
    mode: CopyMode,
    adv: &AdvancedOptions,
    cancel: Arc<AtomicBool>,
    mut report: impl FnMut(u64, &str, ScanTotals),
) -> std::io::Result<Option<ScanOutcome>> {
    let mut child = spawn_robocopy(src, dst, mode.robocopy_flag(), &scan_args(mode, adv))?;
    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = ConsoleLines::new(stdout);
    let parser = LineParser::new(dst);

    let mut found = ScanTotals::default();
    let mut dirs_scanned = 0u64;
    let mut current_dir = String::new();
    let mut last_report = tokio::time::Instant::now();
    let mut errors = ErrorLog::default();
    let mut tail: Vec<String> = Vec::new();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(None);
        }
        if last_report.elapsed() >= SCAN_REPORT_INTERVAL {
            last_report = tokio::time::Instant::now();
            report(dirs_scanned, &current_dir, found);
        }
        // Short timeout so `cancel` is checked, and progress reported, even
        // while robocopy is slow to write the next line.
        let next = tokio::time::timeout(Duration::from_millis(150), reader.next_line());
        let line = match next.await {
            Ok(line) => match line? {
                Some(line) => line,
                None => break,
            },
            Err(_) => continue,
        };
        errors.push(&line);
        match parser.parse(&line) {
            RcLine::File { bytes, .. } => {
                found.files_to_copy += 1;
                found.bytes_to_copy += bytes;
            }
            // Without /PURGE robocopy lists extras but leaves them alone.
            RcLine::Extra if mode == CopyMode::Mirror => found.files_to_delete += 1,
            RcLine::Extra => {}
            RcLine::Dir { path } => {
                dirs_scanned += 1;
                current_dir = path;
            }
            RcLine::Other(line) => {
                tail.push(line);
                if tail.len() > 20 {
                    tail.remove(0);
                }
            }
        }
    }
    report(dirs_scanned, "", found);
    let status = child.wait().await?;
    let code = status.code().unwrap_or(-1);
    let error = errors.summary();
    // 16 (or no code at all) means robocopy could not get at the source or
    // destination: an empty scan must not be taken as "no changes", or a
    // scheduled task would report success without copying anything.
    if !(0..16).contains(&code) {
        return Err(std::io::Error::other(error.unwrap_or_else(|| {
            format!("robocopy exited with code {code} while scanning (fatal error)")
        })));
    }
    // 8 means some files or folders, on either side, could not be read.
    // Robocopy can also report an error and carry on with a lower code: a
    // destination share it cannot reach exits with 1 and lists every file as
    // new. Either way the preview has to say so; the copy still goes ahead,
    // since failing the job over one unreadable folder would leave
    // everything else uncopied, and reports what it could not do.
    let warning = match error {
        Some(error) => Some(format!("robocopy reported a problem while scanning: {error}")),
        // With /NDL an unreadable folder gets no error line while scanning;
        // the copy does print one.
        None if code & 8 != 0 => Some(
            "some files or folders could not be read and will be skipped (the copy's log names them)".into(),
        ),
        None => None,
    };
    // A folder line does not say whether the folder is new (/NC drops the
    // class), so new folders, empty ones included, come from the summary's
    // "Dirs" row (Copied column).
    found.dirs_to_create = parse_summary(&tail, code).map_or(0, |s| s.dirs_copied);
    Ok(Some(ScanOutcome {
        totals: found,
        warning,
    }))
}

pub fn start_copy(
    src: &str,
    dst: &str,
    mode: CopyMode,
    adv: &AdvancedOptions,
) -> std::io::Result<Child> {
    spawn_robocopy(src, dst, mode.robocopy_flag(), &copy_args(mode, adv))
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

    // Lines as printed by a real robocopy with /BYTES /NP /NC /NDL /FP.
    #[test]
    fn parse_line_file() {
        let parser = LineParser::new(r"D:\mirror");
        match parser.parse("\t  \t\t   12345\tC:\\data\\photo.jpg") {
            RcLine::File { bytes, path } => {
                assert_eq!(bytes, 12345);
                assert_eq!(path, "C:\\data\\photo.jpg");
            }
            _ => panic!("expected a file line"),
        }
    }

    #[test]
    fn parse_line_extra_and_other() {
        // With /NC an extra file looks like a file to copy: only its path,
        // under the destination, gives it away. An extra folder has size -1.
        let parser = LineParser::new(" d:/Mirror/ ");
        assert!(matches!(
            parser.parse("\t  \t\t       2\tD:\\mirror\\old.txt"),
            RcLine::Extra
        ));
        assert!(matches!(
            parser.parse("\t      -1\tD:\\MIRROR\\olddir\\"),
            RcLine::Extra
        ));
        // A sibling that shares the prefix is not the destination.
        assert!(matches!(
            parser.parse("\t  \t\t       2\tD:\\mirror2\\new.txt"),
            RcLine::File { bytes: 2, .. }
        ));
        // Localized (Spanish) robocopy header line.
        assert!(matches!(
            parser.parse("   Origen : C:\\data\\"),
            RcLine::Other(_)
        ));
    }

    /// Runs the real robocopy against a destination with extra files: they
    /// must count as deletions in mirror mode, be ignored in add-only mode,
    /// and never be taken for files to copy.
    #[cfg(windows)]
    #[tokio::test]
    async fn scan_tells_extras_from_copies() {
        let base = std::env::temp_dir().join(format!("nasmirror-scan-{}", uuid::Uuid::new_v4()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(dst.join("olddir")).unwrap();
        std::fs::write(src.join("new.txt"), "1234").unwrap();
        std::fs::write(dst.join("extra.txt"), "12").unwrap();
        std::fs::write(dst.join("olddir").join("y.txt"), "12").unwrap();
        let (src_s, dst_s) = (src.display().to_string(), dst.display().to_string());
        let adv = AdvancedOptions::default();
        let cancel = Arc::new(AtomicBool::new(false));

        let mirror = scan(&src_s, &dst_s, CopyMode::Mirror, &adv, cancel.clone(), |_, _, _| {})
            .await
            .unwrap()
            .unwrap();
        let mirror = mirror.totals;
        assert_eq!((mirror.files_to_copy, mirror.bytes_to_copy), (1, 4));
        // extra.txt, olddir and olddir\y.txt
        assert_eq!(mirror.files_to_delete, 3);

        let add_only = scan(&src_s, &dst_s, CopyMode::Accumulate, &adv, cancel, |_, _, _| {})
            .await
            .unwrap()
            .unwrap()
            .totals;
        assert_eq!((add_only.files_to_copy, add_only.bytes_to_copy), (1, 4));
        assert_eq!(add_only.files_to_delete, 0);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// The scan says where it is while it goes: folders are counted, not
    /// taken for files to copy, and the last report has the final numbers.
    #[cfg(windows)]
    #[tokio::test]
    async fn scan_reports_the_folders_it_goes_through() {
        let base = std::env::temp_dir().join(format!("nasmirror-scan-{}", uuid::Uuid::new_v4()));
        let src = base.join("src");
        for dir in ["a", "b", r"b\c"] {
            std::fs::create_dir_all(src.join(dir)).unwrap();
        }
        std::fs::write(src.join(r"b\c\x.txt"), "123").unwrap();
        let (src_s, dst_s) = (src.display().to_string(), base.join("dst").display().to_string());
        let mut reports: Vec<(u64, String, ScanTotals)> = Vec::new();
        let outcome = scan(
            &src_s,
            &dst_s,
            CopyMode::Mirror,
            &AdvancedOptions::default(),
            Arc::new(AtomicBool::new(false)),
            |dirs, dir, totals| reports.push((dirs, dir.to_string(), totals)),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!((outcome.totals.files_to_copy, outcome.totals.bytes_to_copy), (1, 3));
        // The source and its three folders, all new.
        assert_eq!(outcome.totals.dirs_to_create, 4);
        let (dirs, current, totals) = reports.last().cloned().expect("a final report");
        assert_eq!((dirs, current.as_str(), totals.files_to_copy), (4, "", 1));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn command_lines_read_as_typed() {
        let adv = AdvancedOptions {
            exclude_dirs: vec!["System Volume Information".into()],
            ..AdvancedOptions::default()
        };
        assert_eq!(
            scan_command_line(r"C:\My Files", r"\\nas\copy", CopyMode::Mirror, &adv),
            "robocopy \"C:\\My Files\" \\\\nas\\copy /MIR /BYTES /NP /NC /FP /R:1 /W:2 \
             /XD \"System Volume Information\" /L"
        );
        assert!(copy_command_line(r"C:\a", r"D:\b", CopyMode::Accumulate, &adv)
            .ends_with("/E /BYTES /NP /NC /FP /R:1 /W:2 /XD \"System Volume Information\" /NDL /MT:16"));
    }

    #[test]
    fn parse_line_folder() {
        let parser = LineParser::new(r"D:\mirror");
        assert!(matches!(
            parser.parse("\t       3\tC:\\data\\photos\\"),
            RcLine::Dir { path } if path == "C:\\data\\photos\\"
        ));
    }

    /// A new empty folder is the only difference: with /NDL no line lists
    /// it, and the job used to end as "no changes" without creating it.
    #[cfg(windows)]
    #[tokio::test]
    async fn scan_counts_new_empty_folders() {
        let base = std::env::temp_dir().join(format!("nasmirror-scan-{}", uuid::Uuid::new_v4()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(src.join("empty")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.txt"), "1234").unwrap();
        // Copied with its timestamps, so it is already up to date.
        std::fs::copy(src.join("a.txt"), dst.join("a.txt")).unwrap();
        let (src_s, dst_s) = (src.display().to_string(), dst.display().to_string());
        let adv = AdvancedOptions::default();
        let cancel = Arc::new(AtomicBool::new(false));

        for mode in [CopyMode::Mirror, CopyMode::Accumulate] {
            let totals = scan(&src_s, &dst_s, mode, &adv, cancel.clone(), |_, _, _| {})
                .await
                .unwrap()
                .unwrap()
                .totals;
            assert_eq!((totals.files_to_copy, totals.files_to_delete), (0, 0), "{mode:?}");
            assert_eq!(totals.dirs_to_create, 1, "{mode:?}");
        }

        std::fs::create_dir_all(dst.join("empty")).unwrap();
        let totals = scan(&src_s, &dst_s, CopyMode::Mirror, &adv, cancel, |_, _, _| {})
            .await
            .unwrap()
            .unwrap()
            .totals;
        assert!(totals.is_empty(), "{totals:?}");

        let _ = std::fs::remove_dir_all(&base);
    }

    // Lines as printed by a real Spanish robocopy with /MT: the explanation
    // comes on the next line, and a retry repeats the same error.
    #[test]
    fn error_log_keeps_each_error_once_with_its_explanation() {
        let mut log = ErrorLog::default();
        for line in [
            "\t  \t\t       6\tC:\\src\\locked.txt",
            "2026/10/23 00:37:40 ERROR 32 (0x00000020) Copiando archivo C:\\src\\locked.txt",
            "El proceso no tiene acceso al archivo porque está siendo utilizado por otro proceso.",
            "",
            "\t  \t\t       6\tC:\\src\\locked.txt Reintentando...",
            "2026/10/23 00:37:41 ERROR 32 (0x00000020) Copiando archivo C:\\src\\locked.txt",
            "El proceso no tiene acceso al archivo porque está siendo utilizado por otro proceso.",
            "ERROR: SE SUPERÓ EL LÍMITE DE REINTENTOS.",
        ] {
            log.push(line);
        }
        assert_eq!(
            log.summary().as_deref(),
            Some(
                "Copiando archivo C:\\src\\locked.txt: El proceso no tiene acceso al archivo \
                 porque está siendo utilizado por otro proceso."
            )
        );
    }

    #[test]
    fn error_log_counts_other_errors_and_survives_a_missing_explanation() {
        assert_eq!(ErrorLog::default().summary(), None);
        let mut log = ErrorLog::default();
        for line in [
            "2026/10/23 00:50:11 ERROR 5 (0x00000005) Copying Directory C:\\src\\private\\",
            "Access is denied.",
            // Another thread's file line where the explanation should be.
            "2026/10/23 00:50:12 ERROR 53 (0x00000035) Copying File C:\\src\\a.txt",
            "\t  \t\t       1\tC:\\src\\b.txt",
        ] {
            log.push(line);
        }
        assert_eq!(
            log.summary().as_deref(),
            Some("Copying Directory C:\\src\\private\\: Access is denied. (and 1 more errors, see the log)")
        );
        // A trailing error with no explanation yet still counts.
        let mut log = ErrorLog::default();
        log.push("2026/10/23 00:50:12 ERROR 112 (0x00000070) Copying File D:\\big.iso");
        assert_eq!(log.summary().as_deref(), Some("Copying File D:\\big.iso"));
    }

    /// An error robocopy recovers from on retry is not why the run failed:
    /// the one it gave up on ("retry limit exceeded") is reported first.
    #[test]
    fn error_log_reports_the_error_robocopy_gave_up_on() {
        let mut log = ErrorLog::default();
        for line in [
            "2026/10/23 01:00:00 ERROR 64 (0x00000040) Copying File \\\\nas\\copy\\a.txt",
            "The specified network name is no longer available.",
            "Waiting 2 seconds... Retrying...",
            "\t  \t\t       9\tC:\\src\\a.txt",
            "2026/10/23 01:00:05 ERROR 5 (0x00000005) Copying File C:\\src\\b.txt",
            "Access is denied.",
            "",
            "ERROR: RETRY LIMIT EXCEEDED.",
        ] {
            log.push(line);
        }
        assert_eq!(log.summary().as_deref(), Some("Copying File C:\\src\\b.txt: Access is denied."));
    }

    /// The error word is translated in some languages; the line's shape and
    /// the retry-limit line in the same word still give the reason away.
    #[test]
    fn error_log_reads_translated_robocopy() {
        let mut german = ErrorLog::default();
        for line in [
            "2026/10/23 01:00:00 FEHLER 32 (0x00000020) Datei wird kopiert C:\\src\\offen.txt",
            "Der Prozess kann nicht auf die Datei zugreifen, da sie von einem anderen Prozess verwendet wird.",
            "FEHLER: ÜBERSCHREITUNG DES WIEDERHOLUNGSLIMITS.",
        ] {
            german.push(line);
        }
        assert_eq!(
            german.summary().as_deref(),
            Some(
                "Datei wird kopiert C:\\src\\offen.txt: Der Prozess kann nicht auf die Datei \
                 zugreifen, da sie von einem anderen Prozess verwendet wird."
            )
        );

        let mut french = ErrorLog::default();
        for line in [
            "2026/10/23 01:00:00 ERREUR 5 (0x00000005) Copie du fichier C:\\src\\b.txt",
            "Accès refusé.",
            "ERREUR : LIMITE DE NOUVELLES TENTATIVES DÉPASSÉE.",
        ] {
            french.push(line);
        }
        assert_eq!(french.summary().as_deref(), Some("Copie du fichier C:\\src\\b.txt: Accès refusé."));
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

        assert_eq!(copy_only_args(&adv), ["/NDL", "/MT:16"]);
    }
}
