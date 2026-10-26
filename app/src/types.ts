// Mirrors src-tauri/src/engine/types.rs; keep in sync by hand.

export type CopyMode = 'accumulate' | 'mirror'

export interface Credentials {
  user: string
  password: string
}

export interface WakeOnLan {
  mac: string
  host: string
  timeout_secs: number
}

export interface AdvancedOptions {
  threads: number
  unbuffered_io: boolean
  retries: number
  wait_secs: number
  fat_time_tolerance: boolean
  dst_adjust: boolean
  exclude_dirs: string[]
  exclude_files: string[]
  extra_flags: string[]
}

export type Engine = 'robocopy' | 'restic'

export interface ResticOptions {
  keep_daily: number
  keep_weekly: number
  keep_monthly: number
}

export interface Profile {
  id: string
  name: string
  source: string
  destination: string
  mode: CopyMode
  credentials?: Credentials | null
  wake_on_lan?: WakeOnLan | null
  advanced: AdvancedOptions
  engine: Engine
  restic: ResticOptions
}

export interface JobRequest {
  profile: Profile
  password?: string | null
  restic_password?: string | null
  dry_run: boolean
  /** What the preview found: the confirmed copy starts without scanning again. */
  preview?: ScanTotals | null
}

/** What a robocopy scan found to do. */
export interface ScanTotals {
  files_to_copy: number
  bytes_to_copy: number
  files_to_delete: number
  /** Folders missing from the destination, empty ones included. */
  dirs_to_create: number
  /** Size of the files `files_to_delete` counts (mirror mode). */
  bytes_to_delete: number
}

/** What mirror mode would delete under one item at the top of the destination. */
export interface DeletionGroup {
  name: string
  is_dir: boolean
  files: number
  bytes: number
}

export type JobPhase =
  | 'checking'
  | 'connecting'
  | 'waking_target'
  | 'scanning'
  | 'copying'

export type Outcome =
  | 'no_changes'
  | 'success'
  | 'success_with_mismatches'
  | 'failed'
  | 'cancelled'
  | 'connection_error'

export interface RobocopySummary {
  exit_code: number
  dirs_copied: number
  files_copied: number
  files_failed: number
  files_extra: number
  bytes_copied: number
  bytes_failed: number
}

export interface ResticSummary {
  snapshot_id: string
  files_new: number
  files_changed: number
  files_unmodified: number
  data_added: number
  total_files_processed: number
  total_bytes_processed: number
}

export type EngineSummary =
  | ({ engine: 'robocopy' } & RobocopySummary)
  | ({ engine: 'restic' } & ResticSummary)

export interface JobResult {
  outcome: Outcome
  summary: EngineSummary | null
  error: string | null
  log_path: string
  /** Whether the run wrote a log to show (a failed restic backup has no summary, but a log). */
  has_log: boolean
  elapsed_secs: number
}

/** Mirrors src-tauri/src/history.rs. */
export interface HistoryEntry {
  id: string
  finished_at: string
  /** The job that ran; empty for entries written before it was recorded. */
  profile_id: string
  profile_name: string
  source: string
  destination: string
  engine: Engine
  /** `null` for logs written before run history existed: only the date is known. */
  outcome: Outcome | null
  error: string | null
  elapsed_secs: number
  files_copied: number
  bytes_copied: number
  /** Whether the run wrote a log that can be shown. */
  has_log: boolean
}

export interface ProgressSample {
  job_id: string
  at_ms: number
  bytes_done: number
  bytes_total: number
  files_done: number
  files_total: number
  current_file: string
  /** `current_file` was deleted from the destination (mirror mode), not copied from the source. */
  current_deleted: boolean
  /** Files and folders deleted from the destination so far (mirror mode). */
  items_deleted: number
  bytes_deleted: number
}

export type JobEvent =
  | {
      type: 'phase'
      job_id: string
      phase: JobPhase
      /** What exactly the step does: the paths, the share and user, the command run. */
      detail: string | null
    }
  | ({
      type: 'scan_progress'
      job_id: string
      dirs_scanned: number
      /** Empty once the scan has finished. */
      current_dir: string
    } & ScanTotals)
  | ({
      type: 'scan_result'
      job_id: string
      /** A problem robocopy reported while scanning; what it could read can still be copied. */
      warning: string | null
      /** What mirror mode would delete, by item at the top of the destination, largest first. */
      deletions: DeletionGroup[]
      /** How many items at the top of the destination would be deleted in all; `deletions` names the largest. */
      deletion_groups: number
      /** Why these deletions need an explicit yes before copying; `null` when routine. */
      confirm_deletions: string | null
    } & ScanTotals)
  | ({ type: 'progress' } & ProgressSample)
  | { type: 'finished'; job_id: string; result: JobResult }

export function emptyAdvanced(): AdvancedOptions {
  return {
    threads: 16,
    unbuffered_io: false,
    retries: 1,
    wait_secs: 2,
    fat_time_tolerance: false,
    dst_adjust: false,
    exclude_dirs: [],
    exclude_files: [],
    extra_flags: [],
  }
}

export function emptyRestic(): ResticOptions {
  return { keep_daily: 7, keep_weekly: 4, keep_monthly: 6 }
}

export function newProfile(): Profile {
  return {
    id: crypto.randomUUID(),
    name: '',
    source: '',
    destination: '',
    mode: 'accumulate',
    credentials: null,
    wake_on_lan: null,
    advanced: emptyAdvanced(),
    engine: 'robocopy',
    restic: emptyRestic(),
  }
}
