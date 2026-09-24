use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::types::{Engine, EngineSummary, JobResult, Outcome, Profile};

/// A finished run, as shown on the history screen.
///
/// Stored as a `.json` next to its `.log`. Reconstructing the outcome by parsing
/// the robocopy/restic log would be fragile (format differs per engine and
/// system language, and lines are truncated if the run was cancelled).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// File name without extension (`backup_2026-09-23_21-05-00`), so
    /// alphabetical order is also chronological order.
    pub id: String,
    pub finished_at: String,
    pub profile_name: String,
    pub source: String,
    pub destination: String,
    pub engine: Engine,
    /// `None` for logs written before history entries existed; only the date
    /// in their file name is known.
    pub outcome: Option<Outcome>,
    pub error: Option<String>,
    pub elapsed_secs: f64,
    pub files_copied: u64,
    pub bytes_copied: u64,
    /// Whether there is a `.log` to show. A job that fails before the copy
    /// starts (Wake-on-LAN, connection, restic password) never writes one.
    pub has_log: bool,
}

/// Records a real run. Dry runs are not recorded: they copy nothing and would
/// only add noise to the history.
pub fn record(log_path: &Path, profile: &Profile, result: &JobResult) {
    let Some(stem) = log_path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return;
    };
    let (files_copied, bytes_copied) = match &result.summary {
        Some(EngineSummary::Robocopy(s)) => (s.files_copied, s.bytes_copied),
        Some(EngineSummary::Restic(s)) => (s.files_new + s.files_changed, s.data_added),
        None => (0, 0),
    };
    let entry = HistoryEntry {
        id: stem,
        finished_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        profile_name: profile.name.clone(),
        source: profile.source.clone(),
        destination: profile.destination.clone(),
        engine: profile.engine,
        outcome: Some(result.outcome),
        error: result.error.clone(),
        elapsed_secs: result.elapsed_secs,
        files_copied,
        bytes_copied,
        has_log: log_path.is_file(),
    };
    let Ok(text) = serde_json::to_string_pretty(&entry) else {
        return;
    };
    if let Some(dir) = log_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(log_path.with_extension("json"), text);
}

/// Lists the history, newest run first.
pub fn list(log_dir: &Path) -> Vec<HistoryEntry> {
    let Ok(read_dir) = std::fs::read_dir(log_dir) else {
        return vec![];
    };
    let mut entries: Vec<HistoryEntry> = Vec::new();
    let mut orphans: Vec<String> = Vec::new();
    let mut known: HashSet<String> = HashSet::new();

    for file in read_dir.flatten() {
        let path = file.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        match ext {
            "json" => {
                if let Some(entry) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<HistoryEntry>(&t).ok())
                {
                    known.insert(stem);
                    entries.push(entry);
                }
            }
            "log" => orphans.push(stem),
            _ => {}
        }
    }

    // Logs without a `.json` entry (written by older versions) are still listed,
    // with the date from the file name and no outcome.
    for stem in orphans {
        if known.contains(&stem) {
            continue;
        }
        entries.push(HistoryEntry {
            finished_at: date_from_stem(&stem).unwrap_or_default(),
            id: stem,
            profile_name: String::new(),
            source: String::new(),
            destination: String::new(),
            engine: Engine::Robocopy,
            outcome: None,
            error: None,
            elapsed_secs: 0.0,
            files_copied: 0,
            bytes_copied: 0,
            has_log: true,
        });
    }

    entries.sort_by(|a, b| b.id.cmp(&a.id));
    entries
}

/// `backup_2026-09-23_21-05-00` -> `2026-09-23 21:05:00`. A trailing
/// tie-breaker (`..._2`, for two runs in the same second) is ignored.
fn date_from_stem(stem: &str) -> Option<String> {
    let rest = stem.strip_prefix("backup_")?;
    let (date, rest) = rest.split_once('_')?;
    let time = rest.split('_').next()?;
    if date.len() != 10 || time.len() != 8 {
        return None;
    }
    Some(format!("{date} {}", time.replace('-', ":")))
}

/// Resolves `<id>.<ext>` inside `log_dir`. The frontend sends only the entry
/// id, never a path, so a tampered value cannot reach files elsewhere on disk.
fn entry_file(log_dir: &Path, id: &str, ext: &str) -> Result<PathBuf, String> {
    let invalid = || "invalid log id".to_string();
    if id.is_empty() || id == "." || id == ".." {
        return Err(invalid());
    }
    let name = format!("{id}.{ext}");
    let file_name = Path::new(&name).file_name().ok_or_else(invalid)?;
    if file_name != std::ffi::OsStr::new(&name) {
        return Err(invalid());
    }
    Ok(log_dir.join(file_name))
}

/// Returns the log contents of a run.
pub fn read_log(log_dir: &Path, id: &str) -> Result<String, String> {
    let path = entry_file(log_dir, id, "log")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read the log: {e}"))?;
    // Logs of large copies can be tens of MB; the detail view only needs the
    // tail, which holds the summary.
    const MAX_CHARS: usize = 200_000;
    let total = text.chars().count();
    if total > MAX_CHARS {
        let tail: String = text.chars().skip(total - MAX_CHARS).collect();
        return Ok(format!(
            "[...log trimmed: showing the last {MAX_CHARS} characters...]\n{tail}"
        ));
    }
    Ok(text)
}

/// Deletes a run from the history: its `.log` and its `.json` entry. Either may
/// be missing (a failure before copying leaves no log, and an old log has no
/// entry), so it is only an error if neither existed.
pub fn delete(log_dir: &Path, id: &str) -> Result<(), String> {
    let mut removed = false;
    let mut last_error = None;
    for ext in ["log", "json"] {
        let path = entry_file(log_dir, id, ext)?;
        if !path.exists() {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(e) => last_error = Some(format!("could not delete: {e}")),
        }
    }
    match last_error {
        Some(e) => Err(e),
        None if removed => Ok(()),
        None => Err("that history entry no longer exists".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_from_log_name() {
        assert_eq!(
            date_from_stem("backup_2026-09-23_21-05-00").as_deref(),
            Some("2026-09-23 21:05:00")
        );
        assert_eq!(
            date_from_stem("backup_2026-09-23_21-05-00_2").as_deref(),
            Some("2026-09-23 21:05:00")
        );
        assert_eq!(date_from_stem("something-else"), None);
        assert_eq!(date_from_stem("backup_2026-09-23"), None);
    }

    #[test]
    fn read_log_rejects_ids_that_escape_the_log_dir() {
        let dir = std::env::temp_dir().join(format!("nasmirror-hist-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for id in [r"..\..\profiles", r"C:\Windows\win", "..", ".", "", "sub/other"] {
            assert!(read_log(&dir, id).is_err(), "should reject {id:?}");
            assert!(delete(&dir, id).is_err(), "should reject {id:?}");
        }
        // A valid id whose log does not exist fails on read, not on the id.
        assert!(read_log(&dir, "backup_2026-01-01_00-00-00")
            .unwrap_err()
            .contains("could not read the log"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_both_files_and_complains_only_if_there_was_nothing() {
        let dir = std::env::temp_dir().join(format!("nasmirror-hist-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let id = "backup_2026-03-03_10-00-00";
        std::fs::write(dir.join(format!("{id}.log")), "x").unwrap();
        std::fs::write(dir.join(format!("{id}.json")), "{}").unwrap();
        assert!(delete(&dir, id).is_ok());
        assert!(!dir.join(format!("{id}.log")).exists());
        assert!(!dir.join(format!("{id}.json")).exists());
        assert!(delete(&dir, id).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_mixes_entries_and_old_logs_newest_first() {
        let dir = std::env::temp_dir().join(format!("nasmirror-hist-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("backup_2026-01-01_10-00-00.log"), "old").unwrap();
        std::fs::write(dir.join("backup_2026-02-02_10-00-00.log"), "new").unwrap();
        std::fs::write(
            dir.join("backup_2026-02-02_10-00-00.json"),
            serde_json::to_string(&HistoryEntry {
                id: "backup_2026-02-02_10-00-00".into(),
                finished_at: "2026-02-02 10:00:05".into(),
                profile_name: "Photos".into(),
                source: r"C:\photos".into(),
                destination: r"\\nas\backup".into(),
                engine: Engine::Robocopy,
                outcome: Some(Outcome::Success),
                error: None,
                elapsed_secs: 5.0,
                files_copied: 3,
                bytes_copied: 1024,
                has_log: true,
            })
            .unwrap(),
        )
        .unwrap();

        let entries = list(&dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].profile_name, "Photos");
        assert_eq!(entries[0].outcome, Some(Outcome::Success));
        // The old log is listed with its date and no outcome.
        assert_eq!(entries[1].outcome, None);
        assert_eq!(entries[1].finished_at, "2026-01-01 10:00:00");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
