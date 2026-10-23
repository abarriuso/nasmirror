import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { api, onJobEvent } from './api'
import type { HistoryEntry, JobEvent, JobPhase, JobResult, Outcome, Profile, ScanTotals } from './types'
import { ProfileForm } from './components/ProfileForm'
import { JobView } from './components/JobView'
import { JobSteps, type JobStep, type ScanSnapshot, type StepEnd } from './components/JobSteps'
import { ResultsPanel } from './components/ResultsPanel'
import { HistoryPanel } from './components/HistoryPanel'
import { OUTCOME_BADGE } from './outcomes'
import { notifyFinished } from './notify'
import { askToConfirm, showError } from './dialogs'
import type { SpeedSample } from './components/SpeedChart'
import { copyProgress, formatBytes, formatDuration, formatElapsed, formatWhen, plural } from './format'
import './App.css'

type View =
  | { kind: 'list' }
  | { kind: 'history' }
  | { kind: 'form'; editing?: Profile }
  | { kind: 'analyzing'; profile: Profile }
  | {
      kind: 'preview'
      profile: Profile
      password: string
      resticPassword: string
      jobId: string
      scan: ScanTotals
      warning: string | null
    }
  | { kind: 'job'; profile: Profile; jobId: string }
  | { kind: 'result'; result: JobResult; profile: Profile }

interface JobState {
  phase: JobPhase
  /** What the job has done so far, step by step, with timings and details. */
  steps: JobStep[]
  /** How far the scan has got; `null` until it reports. */
  scan: ScanSnapshot | null
  bytesDone: number
  bytesTotal: number
  filesDone: number
  filesTotal: number
  currentFile: string
  samples: SpeedSample[]
}

function emptyJobState(): JobState {
  return {
    phase: 'checking',
    steps: [],
    scan: null,
    bytesDone: 0,
    bytesTotal: 0,
    filesDone: 0,
    filesTotal: 0,
    currentFile: '',
    samples: [],
  }
}

/** Ends the step in progress, if any, as `ended`. */
function closeSteps(steps: JobStep[], now: number, ended: StepEnd = 'done'): JobStep[] {
  return steps.map((s) => (s.endedMs === null ? { ...s, endedMs: now, ended } : s))
}

/** How the step a job ended in went, from the job's outcome. */
function stepEndOf(outcome: Outcome): StepEnd {
  if (outcome === 'failed' || outcome === 'connection_error') return 'failed'
  return outcome === 'cancelled' ? 'stopped' : 'done'
}

export default function App() {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [view, setView] = useState<View>({ kind: 'list' })
  const [job, setJob] = useState<JobState>(emptyJobState())
  const [busy, setBusy] = useState(false)
  const [loadingProfiles, setLoadingProfiles] = useState(true)
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [cancelling, setCancelling] = useState(false)
  const lastBytesRef = useRef<{ atMs: number; bytes: number } | null>(null)
  const activeJobIdRef = useRef<string | null>(null)
  // Cancel pressed before startJob returned the job's id.
  const cancelRequestedRef = useRef(false)

  const refreshHistory = () => {
    api
      .listHistory()
      .then(setHistory)
      .catch(() => {})
  }

  useEffect(() => {
    api
      .listProfiles()
      .then(setProfiles)
      .catch((e) => showError(errorMessage(e)))
      .finally(() => setLoadingProfiles(false))
    refreshHistory()
    const unlisten = onJobEvent(handleJobEvent)
    return () => {
      unlisten.then((fn) => fn())
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  function handleJobEvent(event: JobEvent) {
    const activeJobId = activeJobIdRef.current
    // Filter every event by job_id so a straggling job cannot leak into
    // another job's UI.
    if ('job_id' in event && activeJobId && event.job_id !== activeJobId) return

    switch (event.type) {
      case 'phase': {
        const now = Date.now()
        setJob((j) => {
          const last = j.steps[j.steps.length - 1]
          // The same step again (restic opening, then creating, the
          // repository): only its detail changes.
          const steps =
            last && last.endedMs === null && last.phase === event.phase
              ? [...j.steps.slice(0, -1), { ...last, detail: event.detail }]
              : [
                  ...closeSteps(j.steps, now),
                  { phase: event.phase, detail: event.detail, startedMs: now, endedMs: null },
                ]
          return { ...j, phase: event.phase, steps }
        })
        break
      }
      case 'scan_progress':
        setJob((j) => ({
          ...j,
          scan: {
            dirsScanned: event.dirs_scanned,
            currentDir: event.current_dir,
            totals: {
              files_to_copy: event.files_to_copy,
              bytes_to_copy: event.bytes_to_copy,
              files_to_delete: event.files_to_delete,
              dirs_to_create: event.dirs_to_create,
            },
          },
        }))
        break
      case 'scan_result':
        setJob((j) => ({
          ...j,
          bytesTotal: event.bytes_to_copy,
          filesTotal: event.files_to_copy,
        }))
        pendingScan.current = {
          scan: {
            files_to_copy: event.files_to_copy,
            bytes_to_copy: event.bytes_to_copy,
            files_to_delete: event.files_to_delete,
            dirs_to_create: event.dirs_to_create,
          },
          warning: event.warning,
        }
        // The final numbers, folders to create included, for the step list.
        setJob((j) => ({
          ...j,
          scan: {
            dirsScanned: j.scan?.dirsScanned ?? 0,
            currentDir: '',
            totals: pendingScan.current!.scan,
          },
        }))
        break
      case 'progress': {
        const now = Date.now()
        const last = lastBytesRef.current
        let bytesPerSec = 0
        if (last && now > last.atMs) {
          bytesPerSec = Math.max(0, ((event.bytes_done - last.bytes) / (now - last.atMs)) * 1000)
        }
        lastBytesRef.current = { atMs: now, bytes: event.bytes_done }
        setJob((j) => {
          const bytesTotal = Math.max(j.bytesTotal, event.bytes_total)
          const filesTotal = Math.max(j.filesTotal, event.files_total)
          const progress = copyProgress(event.bytes_done, bytesTotal, event.files_done, filesTotal)
          const samples = [...j.samples, { tMs: now, bytesPerSec, progress }]
          const cutoff = now - 65_000
          while (samples.length > 1 && samples[0].tMs < cutoff) samples.shift()
          return {
            ...j,
            bytesDone: event.bytes_done,
            bytesTotal,
            filesDone: event.files_done,
            filesTotal,
            currentFile: event.current_file || j.currentFile,
            samples,
          }
        })
        break
      }
      case 'finished': {
        const now = Date.now()
        const ended = stepEndOf(event.result.outcome)
        setJob((j) => ({ ...j, steps: closeSteps(j.steps, now, ended) }))
        onJobFinished(event.result)
        break
      }
      default:
        break
    }
  }

  const pendingScan = useRef<{ scan: ScanTotals; warning: string | null } | null>(null)
  const pendingContext = useRef<{
    profile: Profile
    password: string
    resticPassword: string
    forReal: boolean
  } | null>(null)

  function onJobFinished(result: JobResult) {
    const ctx = pendingContext.current
    if (!ctx) return
    setBusy(false)
    setCancelling(false)
    if (!ctx.forReal) {
      pendingContext.current = null
      if (result.outcome === 'cancelled') {
        activeJobIdRef.current = null
        setView({ kind: 'list' })
        return
      }
      // The scan only leads to the preview if it found changes; "no changes"
      // or any error is shown directly as the result.
      if (result.outcome !== 'success') {
        setView({ kind: 'result', result, profile: ctx.profile })
        return
      }
      const found = pendingScan.current ?? {
        scan: { files_to_copy: 0, bytes_to_copy: 0, files_to_delete: 0, dirs_to_create: 0 },
        warning: null,
      }
      setView({
        kind: 'preview',
        profile: ctx.profile,
        password: ctx.password,
        resticPassword: ctx.resticPassword,
        jobId: activeJobIdRef.current ?? '',
        scan: found.scan,
        warning: found.warning,
      })
      return
    }
    pendingContext.current = null
    activeJobIdRef.current = null
    void notifyFinished(ctx.profile.name, result)
    setView({ kind: 'result', result, profile: ctx.profile })
    refreshHistory()
  }

  async function startPreview(profile: Profile, password: string, resticPassword: string) {
    if (busy) return
    setBusy(true)
    lastBytesRef.current = null
    pendingContext.current = { profile, password, resticPassword, forReal: false }
    pendingScan.current = null
    // Until startJob returns the id, events from the new job must not be
    // filtered by the previous job's id (the backend runs one job at a time).
    activeJobIdRef.current = null
    cancelRequestedRef.current = false
    setCancelling(false)
    if (profile.engine === 'restic') {
      await confirmAndRun({ profile, password, resticPassword })
      return
    }
    // The scan can take a while (Wake-on-LAN, connecting, large trees), so
    // its steps are shown as they go and it can be cancelled.
    setJob(emptyJobState())
    setView({ kind: 'analyzing', profile })
    try {
      const jobId = await api.startJob({ profile, password: password || null, dry_run: true })
      activeJobIdRef.current = jobId
      if (cancelRequestedRef.current) await api.cancelJob(jobId)
    } catch (e: unknown) {
      setBusy(false)
      setView({ kind: 'list' })
      showError(errorMessage(e))
    }
  }

  async function cancelAnalysis() {
    setCancelling(true)
    const jobId = activeJobIdRef.current
    // Pressed before the job had an id: `startPreview` cancels it as soon
    // as it gets one.
    if (jobId) await api.cancelJob(jobId)
    else cancelRequestedRef.current = true
  }

  async function confirmAndRun(fromPreview?: { profile: Profile; password: string; resticPassword: string }) {
    let profile: Profile, password: string, resticPassword: string
    let preview: ScanTotals | null = null
    if (fromPreview) {
      ;({ profile, password, resticPassword } = fromPreview)
    } else {
      if (view.kind !== 'preview') return
      ;({ profile, password, resticPassword } = view)
      preview = view.scan
    }
    setJob(emptyJobState())
    lastBytesRef.current = null
    setBusy(true)
    setCancelling(false)
    const context = { profile, password, resticPassword, forReal: true }
    pendingContext.current = context
    activeJobIdRef.current = null
    try {
      const jobId = await api.startJob({
        profile,
        password: password || null,
        restic_password: resticPassword || null,
        dry_run: false,
        preview,
      })
      // A job that fails at once (a missing source, say) can deliver
      // `finished` before startJob returns: its result is already on screen
      // and must not be replaced by a progress view that never ends.
      if (pendingContext.current !== context) return
      activeJobIdRef.current = jobId
      setView({ kind: 'job', profile, jobId })
    } catch (e: unknown) {
      setBusy(false)
      showError(errorMessage(e))
    }
  }

  async function cancelJob() {
    if (view.kind !== 'job') return
    const { jobId } = view
    if (!(await askToConfirm('Stop the copy in progress? Files already copied are kept.', 'Stop'))) return
    setCancelling(true)
    await api.cancelJob(jobId)
  }

  async function saveProfile(profile: Profile) {
    try {
      setProfiles(await api.saveProfile(profile))
      setView({ kind: 'list' })
    } catch (e: unknown) {
      showError(`Could not save the job: ${errorMessage(e)}`)
    }
  }

  async function deleteProfile(profile: Profile) {
    if (!(await askToConfirm(`Delete the job "${profile.name}"? Copied files are not removed.`, 'Delete')))
      return
    try {
      setProfiles(await api.deleteProfile(profile.id))
    } catch (e: unknown) {
      showError(`Could not delete the job: ${errorMessage(e)}`)
    }
  }

  return (
    <div className="app-shell">
      <header className="app-header">
        <span className="app-header__brand">NASMirror</span>
        <span className="app-header__tag">mirror folders to NAS and drives</span>
        <button
          className="link-btn app-header__logs"
          disabled={busy}
          onClick={() => setView({ kind: 'history' })}
        >
          History
        </button>
      </header>

      <main className="app-main">
        {view.kind === 'history' && (
          <HistoryPanel
            onClose={() => {
              // Entries may have been deleted: the job cards show the latest.
              refreshHistory()
              setView({ kind: 'list' })
            }}
          />
        )}

        {view.kind === 'list' && (
          <ProfileList
            profiles={profiles}
            history={history}
            busy={busy}
            loading={loadingProfiles}
            onNew={() => setView({ kind: 'form' })}
            onEdit={(p) => setView({ kind: 'form', editing: p })}
            onDelete={deleteProfile}
            onRun={startPreview}
          />
        )}

        {view.kind === 'form' && (
          <ProfileForm
            initial={view.editing}
            onCancel={() => setView({ kind: 'list' })}
            onSave={saveProfile}
          />
        )}

        {view.kind === 'analyzing' && (
          <div className="preview-panel">
            <h2>{view.profile.name}</h2>
            <Route source={view.profile.source} destination={view.profile.destination} />
            <JobSteps steps={job.steps} scan={job.scan} source={view.profile.source} />
            {job.phase === 'scanning' && (
              <p className="field-hint">
                Nothing is copied or deleted yet. Robocopy is comparing every file in the source
                with the destination; on a large folder over the network that takes a while, and
                you will see the result before anything is done.
              </p>
            )}
            <div className="form-actions">
              <button type="button" disabled={cancelling} onClick={cancelAnalysis}>
                {cancelling ? 'Cancelling…' : 'Cancel'}
              </button>
            </div>
          </div>
        )}

        {view.kind === 'preview' && (
          <PreviewPanel
            scan={view.scan}
            warning={view.warning}
            source={view.profile.source}
            destination={view.profile.destination}
            mirror={view.profile.mode === 'mirror'}
            steps={job.steps}
            scanSnapshot={job.scan}
            onCancel={() => { setBusy(false); setView({ kind: 'list' }) }}
            onConfirm={() => confirmAndRun()}
          />
        )}

        {view.kind === 'job' && (
          <JobView
            phase={job.phase}
            bytesDone={job.bytesDone}
            bytesTotal={job.bytesTotal}
            filesDone={job.filesDone}
            filesTotal={job.filesTotal}
            currentFile={job.currentFile}
            samples={job.samples}
            steps={job.steps}
            source={view.profile.source}
            stopping={cancelling}
            onCancel={cancelJob}
          />
        )}

        {view.kind === 'result' && (
          <ResultsPanel
            result={view.result}
            profile={view.profile}
            steps={job.steps}
            onClose={() => setView({ kind: 'list' })}
            onOpenLogDir={() => api.openLogDir()}
          />
        )}
      </main>
    </div>
  )
}

function ProfileList({
  profiles,
  busy,
  loading,
  onNew,
  onEdit,
  onDelete,
  onRun,
  history,
}: {
  profiles: Profile[]
  /** Newest first, for each card's last run. */
  history: HistoryEntry[]
  busy: boolean
  loading: boolean
  onNew: () => void
  onEdit: (p: Profile) => void
  onDelete: (p: Profile) => void
  onRun: (p: Profile, password: string, resticPassword: string) => void
}) {
  const [runningPasswordFor, setRunningPasswordFor] = useState<Profile | null>(null)
  const [password, setPassword] = useState('')
  const [resticPassword, setResticPassword] = useState('')

  const closePasswords = () => {
    setRunningPasswordFor(null)
    setPassword('')
    setResticPassword('')
  }
  const runWithPasswords = (p: Profile) => {
    if (busy) return
    onRun(p, password, resticPassword)
    closePasswords()
  }
  // Enter runs the job from either password field; Escape backs out.
  const passwordKeys = (p: Profile) => (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') runWithPasswords(p)
    else if (e.key === 'Escape') closePasswords()
  }

  return (
    <div className="profile-list">
      <div className="profile-list__toolbar">
        <h1>Sync jobs</h1>
        <button className="primary" onClick={onNew}>
          + New job
        </button>
      </div>
      {/* Placeholder cards while profiles load from disk, so the "no saved
          jobs" message does not flash before the existing jobs appear. */}
      {loading && (
        <div aria-busy="true" aria-live="polite">
          <span className="sr-only">Loading jobs…</span>
          <div className="skeleton-card" />
          <div className="skeleton-card" />
        </div>
      )}
      {!loading && profiles.length === 0 && (
        <p className="profile-list__empty">
          No saved jobs yet. Create one with the button above.
        </p>
      )}
      <ul className="profile-cards" role="list">
        {profiles.map((p) => (
          <li key={p.id} className="profile-card">
            <div className="profile-card__main">
              <strong>{p.name}</strong>
              {/* One line each: on a single line a long source pushed the
                  destination out of sight. */}
              <span className="profile-card__paths" title={p.source}>
                {p.source}
              </span>
              <span className="profile-card__paths" title={p.destination}>
                → {p.destination}
              </span>
              <div className="profile-card__badges">
                <span className={`profile-card__mode profile-card__mode--${p.mode}`}>
                  {p.mode === 'mirror' ? 'Mirror' : 'Add only'}
                </span>
                <span className={`profile-card__mode profile-card__mode--${p.engine}`}>
                  {p.engine === 'restic' ? 'Versioned' : 'Direct copy'}
                </span>
              </div>
              <LastRun entry={lastRunOf(p, history)} />
            </div>
            <div className="profile-card__actions">
              {runningPasswordFor?.id === p.id ? (
                <>
                  {p.credentials && (
                    <input
                      autoFocus
                      type="password"
                      placeholder="Network password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      onKeyDown={passwordKeys(p)}
                    />
                  )}
                  {p.engine === 'restic' && (
                    <input
                      autoFocus={!p.credentials}
                      type="password"
                      placeholder="Encryption password"
                      value={resticPassword}
                      onChange={(e) => setResticPassword(e.target.value)}
                      onKeyDown={passwordKeys(p)}
                    />
                  )}
                  <button className="primary" disabled={busy} onClick={() => runWithPasswords(p)}>
                    {p.engine === 'restic' ? 'Back up' : 'Preview'}
                  </button>
                  <button type="button" onClick={closePasswords}>
                    Cancel
                  </button>
                </>
              ) : (
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => {
                    if (p.credentials || p.engine === 'restic') {
                      setRunningPasswordFor(p)
                    } else {
                      onRun(p, '', '')
                    }
                  }}
                >
                  Start
                </button>
              )}
              <button onClick={() => onEdit(p)}>Edit</button>
              <button className="danger" onClick={() => onDelete(p)}>
                Delete
              </button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}

/**
 * The job's latest run in the history: by id, or by name for entries written
 * before the id was recorded.
 */
function lastRunOf(p: Profile, history: HistoryEntry[]): HistoryEntry | undefined {
  return history.find((e) => (e.profile_id ? e.profile_id === p.id : e.profile_name === p.name))
}

function LastRun({ entry }: { entry: HistoryEntry | undefined }) {
  if (!entry?.outcome) return <span className="profile-card__last">Never run</span>
  const badge = OUTCOME_BADGE[entry.outcome]
  return (
    <span className="profile-card__last" title={entry.error ?? undefined}>
      Last run {formatWhen(entry.finished_at)} ·{' '}
      <span className={`profile-card__last-outcome profile-card__last-outcome--${badge.tone}`}>
        {badge.label}
      </span>
      {entry.outcome !== 'failed' && entry.outcome !== 'connection_error' && (
        <>
          {' '}
          · {plural(entry.files_copied, 'file')} · {formatBytes(entry.bytes_copied)} ·{' '}
          {formatDuration(entry.elapsed_secs)}
        </>
      )}
    </span>
  )
}

function Route({ source, destination }: { source: string; destination: string }) {
  return (
    <div className="preview-panel__route">
      <code className="preview-panel__path">{source}</code>
      <span className="preview-panel__arrow" aria-label="to">
        →
      </span>
      <code className="preview-panel__path">{destination}</code>
    </div>
  )
}

function PreviewPanel({
  scan,
  warning,
  source,
  destination,
  mirror,
  steps,
  scanSnapshot,
  onCancel,
  onConfirm,
}: {
  scan: ScanTotals
  warning: string | null
  source: string
  destination: string
  mirror: boolean
  /** How the preview was worked out, for its details. */
  steps: JobStep[]
  scanSnapshot: ScanSnapshot | null
  onCancel: () => void
  onConfirm: () => void
}) {
  const filesToDelete = scan.files_to_delete
  const scanStep = steps.find((s) => s.phase === 'scanning')
  return (
    <div className="preview-panel">
      <h2>Before copying</h2>
      <p>
        <strong>{plural(scan.files_to_copy, 'file')}</strong> will be copied (
        {formatBytes(scan.bytes_to_copy)})
        {scan.dirs_to_create > 0 && (
          <>
            {' '}
            and <strong>{plural(scan.dirs_to_create, 'new folder')}</strong> created
          </>
        )}
        :
      </p>
      <Route source={source} destination={destination} />
      {mirror && filesToDelete > 0 && (
        <p className="preview-panel__danger">
          ⚠ Mirror mode:{' '}
          <strong>{plural(filesToDelete, 'file or folder', 'files or folders')}</strong> will be
          <strong> deleted</strong> from the destination because{' '}
          {filesToDelete === 1 ? 'it no longer exists' : 'they no longer exist'} in the source.
        </p>
      )}
      {warning && <p className="preview-panel__danger">⚠ {warning}</p>}
      {scanStep?.endedMs != null && (
        <p className="field-hint">
          Compared {plural(scanSnapshot?.dirsScanned ?? 0, 'folder')} in{' '}
          {formatElapsed(scanStep.endedMs - scanStep.startedMs)}. Nothing has been copied yet.
        </p>
      )}
      <details className="details">
        <summary>How this was worked out</summary>
        <JobSteps steps={steps} scan={scanSnapshot} source={source} />
      </details>
      <div className="form-actions">
        <button onClick={onCancel}>Cancel</button>
        <button className={mirror && filesToDelete > 0 ? 'danger' : 'primary'} onClick={onConfirm}>
          Copy now
        </button>
      </div>
    </div>
  )
}

function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}
