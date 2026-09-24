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
}

export type JobPhase =
  | 'connecting'
  | 'waking_target'
  | 'scanning'
  | 'copying'
  | 'cancelling'
  | 'done'

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
  elapsed_secs: number
}

/** Mirrors src-tauri/src/history.rs. */
export interface HistoryEntry {
  id: string
  finished_at: string
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
  current_file_bytes_done: number
  current_file_bytes_total: number
}

export type JobEvent =
  | { type: 'phase'; job_id: string; phase: JobPhase }
  | {
      type: 'scan_result'
      job_id: string
      files_to_copy: number
      bytes_to_copy: number
      files_to_delete: number
    }
  | ({ type: 'progress' } & ProgressSample)
  | { type: 'log_line'; job_id: string; line: string }
  | { type: 'retry_warning'; job_id: string; file: string; attempt: number }
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
