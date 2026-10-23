use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyMode {
    /// Adds and updates; never deletes anything at the destination.
    Accumulate,
    /// Makes the destination identical to the source (deletes extra files).
    Mirror,
}

impl CopyMode {
    pub fn robocopy_flag(self) -> &'static str {
        match self {
            CopyMode::Accumulate => "/E",
            CopyMode::Mirror => "/MIR",
        }
    }
}

/// Which tool performs the copy, chosen per job. Robocopy is fast and simple
/// but copies whole files with no history; restic adds deduplication,
/// encryption and versioned snapshots, at the cost of speed and an external
/// binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    #[default]
    Robocopy,
    Restic,
}

/// Restic settings for a job. Like the network password, the repository
/// password is never persisted; it is requested on every run.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResticOptions {
    #[serde(default)]
    pub keep_daily: u32,
    #[serde(default)]
    pub keep_weekly: u32,
    #[serde(default)]
    pub keep_monthly: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub user: String,
    /// Never persisted in the profile saved to disk.
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeOnLan {
    pub mac: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_wol_timeout")]
    pub timeout_secs: u32,
}
fn default_wol_timeout() -> u32 {
    120
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedOptions {
    #[serde(default = "default_threads")]
    pub threads: u32,
    #[serde(default)]
    pub unbuffered_io: bool,
    #[serde(default = "default_retries")]
    pub retries: u32,
    #[serde(default = "default_wait")]
    pub wait_secs: u32,
    #[serde(default)]
    pub fat_time_tolerance: bool,
    #[serde(default)]
    pub dst_adjust: bool,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    #[serde(default)]
    pub exclude_files: Vec<String>,
    #[serde(default)]
    pub extra_flags: Vec<String>,
}
fn default_threads() -> u32 {
    16
}
fn default_retries() -> u32 {
    1
}
fn default_wait() -> u32 {
    2
}

impl Default for AdvancedOptions {
    fn default() -> Self {
        Self {
            threads: default_threads(),
            unbuffered_io: false,
            retries: default_retries(),
            wait_secs: default_wait(),
            fat_time_tolerance: false,
            dst_adjust: false,
            exclude_dirs: vec![],
            exclude_files: vec![],
            extra_flags: vec![],
        }
    }
}

/// A named, saved copy job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub source: String,
    pub destination: String,
    pub mode: CopyMode,
    #[serde(default)]
    pub credentials: Option<Credentials>,
    #[serde(default)]
    pub wake_on_lan: Option<WakeOnLan>,
    #[serde(default)]
    pub advanced: AdvancedOptions,
    #[serde(default)]
    pub engine: Engine,
    #[serde(default)]
    pub restic: ResticOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub profile: Profile,
    /// Held only in memory for this request, never in the saved profile.
    #[serde(default)]
    pub password: Option<String>,
    /// Restic repository password (when `profile.engine == Restic`). Never
    /// persisted.
    #[serde(default)]
    pub restic_password: Option<String>,
    /// Scan only (robocopy /L): nothing is copied or deleted.
    #[serde(default)]
    pub dry_run: bool,
    /// What the preview just found, sent back when the user confirms it. The
    /// copy then starts at once instead of scanning the whole tree again,
    /// which on a large source over the network can take minutes.
    #[serde(default)]
    pub preview: Option<ScanTotals>,
}

/// What a robocopy scan (`/L`) found to do.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ScanTotals {
    pub files_to_copy: u64,
    pub bytes_to_copy: u64,
    pub files_to_delete: u64,
    /// Folders missing from the destination, empty ones included: with no
    /// file inside them they show up nowhere else.
    #[serde(default)]
    pub dirs_to_create: u64,
}

impl ScanTotals {
    pub fn is_empty(&self) -> bool {
        self.files_to_copy == 0 && self.files_to_delete == 0 && self.dirs_to_create == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    /// Validating the source and destination paths.
    Checking,
    Connecting,
    WakingTarget,
    Scanning,
    Copying,
    Cancelling,
    Done,
}

/// Why the job ended. For robocopy it is derived from the real exit code
/// (0-7 = success with varying detail, 8+ = failure; see the robocopy docs),
/// never inferred from the progress reaching 100%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    NoChanges,
    Success,
    SuccessWithMismatches,
    Failed,
    Cancelled,
    ConnectionError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobocopySummary {
    pub exit_code: i32,
    pub dirs_copied: u64,
    pub files_copied: u64,
    pub files_failed: u64,
    pub files_extra: u64,
    pub bytes_copied: u64,
    pub bytes_failed: u64,
}

/// From the `summary` message of `restic backup --json` (message_type: "summary").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResticSummary {
    pub snapshot_id: String,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    pub data_added: u64,
    pub total_files_processed: u64,
    pub total_bytes_processed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "engine", rename_all = "snake_case")]
pub enum EngineSummary {
    Robocopy(RobocopySummary),
    Restic(ResticSummary),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub outcome: Outcome,
    pub summary: Option<EngineSummary>,
    pub error: Option<String>,
    pub log_path: String,
    pub elapsed_secs: f64,
}

/// A single progress sample. The engine sends raw counters rather than
/// pre-averaged values; smoothing and charting are left to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSample {
    pub job_id: String,
    pub at_ms: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub current_file: String,
    pub current_file_bytes_done: u64,
    pub current_file_bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    Phase {
        job_id: String,
        phase: JobPhase,
        /// What exactly this step does, for the window to show: the paths,
        /// the share and user, the command line run.
        #[serde(default)]
        detail: Option<String>,
    },
    /// The scan so far, a few times a second: with no changes to list,
    /// robocopy prints nothing for minutes on a large tree.
    ScanProgress {
        job_id: String,
        dirs_scanned: u64,
        current_dir: String,
        #[serde(flatten)]
        totals: ScanTotals,
    },
    ScanResult {
        job_id: String,
        #[serde(flatten)]
        totals: ScanTotals,
        /// A problem robocopy reported while scanning (part of the tree it
        /// could not read, a destination it could not reach): what it could
        /// read can still be copied.
        warning: Option<String>,
    },
    Progress(ProgressSample),
    LogLine {
        job_id: String,
        line: String,
    },
    RetryWarning {
        job_id: String,
        file: String,
        attempt: u32,
    },
    Finished {
        job_id: String,
        result: JobResult,
    },
}
