import { useEffect, useRef, useState } from 'react'
import { api, onJobEvent } from './api'
import type { JobEvent, JobPhase, JobResult, Profile } from './types'
import { ProfileForm } from './components/ProfileForm'
import { JobView } from './components/JobView'
import { PHASE_LABEL } from './phases'
import { ResultsPanel } from './components/ResultsPanel'
import { HistoryPanel } from './components/HistoryPanel'
import { notifyFinished } from './notify'
import type { SpeedSample } from './components/SpeedChart'
import { formatBytes } from './format'
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
      filesToCopy: number
      bytesToCopy: number
      filesToDelete: number
    }
  | { kind: 'job'; profile: Profile; jobId: string }
  | { kind: 'result'; result: JobResult }

interface JobState {
  phase: JobPhase
  bytesDone: number
  bytesTotal: number
  filesDone: number
  filesTotal: number
  currentFile: string
  samples: SpeedSample[]
}

function emptyJobState(): JobState {
  return {
    phase: 'connecting',
    bytesDone: 0,
    bytesTotal: 0,
    filesDone: 0,
    filesTotal: 0,
    currentFile: '',
    samples: [],
  }
}

export default function App() {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [view, setView] = useState<View>({ kind: 'list' })
  const [job, setJob] = useState<JobState>(emptyJobState())
  const [busy, setBusy] = useState(false)
  const [loadingProfiles, setLoadingProfiles] = useState(true)
  const lastBytesRef = useRef<{ atMs: number; bytes: number } | null>(null)
  const activeJobIdRef = useRef<string | null>(null)

  useEffect(() => {
    api
      .listProfiles()
      .then(setProfiles)
      .catch((e) => alert(errorMessage(e)))
      .finally(() => setLoadingProfiles(false))
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
      case 'phase':
        setJob((j) => ({ ...j, phase: event.phase }))
        break
      case 'scan_result':
        setJob((j) => ({
          ...j,
          bytesTotal: event.bytes_to_copy,
          filesTotal: event.files_to_copy,
        }))
        pendingScan.current = {
          filesToCopy: event.files_to_copy,
          bytesToCopy: event.bytes_to_copy,
          filesToDelete: event.files_to_delete,
        }
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
          const samples = [...j.samples, { tMs: now, bytesPerSec }]
          const cutoff = now - 65_000
          while (samples.length > 1 && samples[0].tMs < cutoff) samples.shift()
          return {
            ...j,
            bytesDone: event.bytes_done,
            bytesTotal: Math.max(j.bytesTotal, event.bytes_total),
            filesDone: event.files_done,
            filesTotal: Math.max(j.filesTotal, event.files_total),
            currentFile: event.current_file || j.currentFile,
            samples,
          }
        })
        break
      }
      case 'finished':
        onJobFinished(event.result)
        break
      default:
        break
    }
  }

  const pendingScan = useRef<{ filesToCopy: number; bytesToCopy: number; filesToDelete: number } | null>(null)
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
        setView({ kind: 'result', result })
        return
      }
      const scan = pendingScan.current ?? { filesToCopy: 0, bytesToCopy: 0, filesToDelete: 0 }
      setView({
        kind: 'preview',
        profile: ctx.profile,
        password: ctx.password,
        resticPassword: ctx.resticPassword,
        jobId: activeJobIdRef.current ?? '',
        filesToCopy: scan.filesToCopy,
        bytesToCopy: scan.bytesToCopy,
        filesToDelete: scan.filesToDelete,
      })
      return
    }
    pendingContext.current = null
    activeJobIdRef.current = null
    void notifyFinished(ctx.profile.name, result)
    setView({ kind: 'result', result })
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
    if (profile.engine === 'restic') {
      await confirmAndRun({ profile, password, resticPassword })
      return
    }
    // The scan can take a while (Wake-on-LAN, connecting, large trees), so
    // its phase is shown and it can be cancelled.
    setJob(emptyJobState())
    setView({ kind: 'analyzing', profile })
    try {
      const jobId = await api.startJob({ profile, password: password || null, dry_run: true })
      activeJobIdRef.current = jobId
    } catch (e: unknown) {
      setBusy(false)
      setView({ kind: 'list' })
      alert(errorMessage(e))
    }
  }

  async function cancelAnalysis() {
    const jobId = activeJobIdRef.current
    if (jobId) await api.cancelJob(jobId)
  }

  async function confirmAndRun(fromPreview?: { profile: Profile; password: string; resticPassword: string }) {
    let profile: Profile, password: string, resticPassword: string
    if (fromPreview) {
      ;({ profile, password, resticPassword } = fromPreview)
    } else {
      if (view.kind !== 'preview') return
      ;({ profile, password, resticPassword } = view)
    }
    setJob(emptyJobState())
    lastBytesRef.current = null
    setBusy(true)
    pendingContext.current = { profile, password, resticPassword, forReal: true }
    activeJobIdRef.current = null
    try {
      const jobId = await api.startJob({
        profile,
        password: password || null,
        restic_password: resticPassword || null,
        dry_run: false,
      })
      activeJobIdRef.current = jobId
      setView({ kind: 'job', profile, jobId })
    } catch (e: unknown) {
      setBusy(false)
      alert(errorMessage(e))
    }
  }

  async function cancelJob() {
    if (view.kind !== 'job') return
    if (!window.confirm('Stop the copy in progress? Files already copied are kept.')) return
    await api.cancelJob(view.jobId)
  }

  async function saveProfile(profile: Profile) {
    try {
      setProfiles(await api.saveProfile(profile))
      setView({ kind: 'list' })
    } catch (e: unknown) {
      alert(`Could not save the job: ${errorMessage(e)}`)
    }
  }

  async function deleteProfile(id: string) {
    if (!window.confirm('Delete this job? Copied files are not removed.')) return
    try {
      setProfiles(await api.deleteProfile(id))
    } catch (e: unknown) {
      alert(`Could not delete the job: ${errorMessage(e)}`)
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
        {view.kind === 'history' && <HistoryPanel onClose={() => setView({ kind: 'list' })} />}

        {view.kind === 'list' && (
          <ProfileList
            profiles={profiles}
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
          <div className="preview-panel" aria-live="polite">
            <h2>{view.profile.name}</h2>
            <p>{PHASE_LABEL[job.phase]}</p>
            <div className="form-actions">
              <button type="button" onClick={cancelAnalysis}>
                Cancel
              </button>
            </div>
          </div>
        )}

        {view.kind === 'preview' && (
          <PreviewPanel
            filesToCopy={view.filesToCopy}
            bytesToCopy={view.bytesToCopy}
            filesToDelete={view.filesToDelete}
            source={view.profile.source}
            destination={view.profile.destination}
            mirror={view.profile.mode === 'mirror'}
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
            onCancel={cancelJob}
          />
        )}

        {view.kind === 'result' && (
          <ResultsPanel
            result={view.result}
            onClose={() => setView({ kind: 'list' })}
            onOpenLog={() => api.openLogDir()}
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
}: {
  profiles: Profile[]
  busy: boolean
  loading: boolean
  onNew: () => void
  onEdit: (p: Profile) => void
  onDelete: (id: string) => void
  onRun: (p: Profile, password: string, resticPassword: string) => void
}) {
  const [runningPasswordFor, setRunningPasswordFor] = useState<Profile | null>(null)
  const [password, setPassword] = useState('')
  const [resticPassword, setResticPassword] = useState('')

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
              <span className="profile-card__paths">
                {p.source} → {p.destination}
              </span>
              <div className="profile-card__badges">
                <span className={`profile-card__mode profile-card__mode--${p.mode}`}>
                  {p.mode === 'mirror' ? 'Mirror' : 'Add only'}
                </span>
                <span className={`profile-card__mode profile-card__mode--${p.engine}`}>
                  {p.engine === 'restic' ? 'Versioned' : 'Direct copy'}
                </span>
              </div>
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
                    />
                  )}
                  {p.engine === 'restic' && (
                    <input
                      autoFocus={!p.credentials}
                      type="password"
                      placeholder="Encryption password"
                      value={resticPassword}
                      onChange={(e) => setResticPassword(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') {
                          onRun(p, password, resticPassword)
                          setRunningPasswordFor(null)
                          setPassword('')
                          setResticPassword('')
                        }
                      }}
                    />
                  )}
                  <button
                    className="primary"
                    disabled={busy}
                    onClick={() => {
                      onRun(p, password, resticPassword)
                      setRunningPasswordFor(null)
                      setPassword('')
                      setResticPassword('')
                    }}
                  >
                    {p.engine === 'restic' ? 'Back up' : 'Preview'}
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
              <button className="danger" onClick={() => onDelete(p.id)}>
                Delete
              </button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}

function PreviewPanel({
  filesToCopy,
  bytesToCopy,
  filesToDelete,
  source,
  destination,
  mirror,
  onCancel,
  onConfirm,
}: {
  filesToCopy: number
  bytesToCopy: number
  filesToDelete: number
  source: string
  destination: string
  mirror: boolean
  onCancel: () => void
  onConfirm: () => void
}) {
  return (
    <div className="preview-panel">
      <h2>Before copying</h2>
      <p>
        <strong>{filesToCopy}</strong> files will be copied ({formatBytes(bytesToCopy)}):
      </p>
      <div className="preview-panel__route">
        <code className="preview-panel__path">{source}</code>
        <span className="preview-panel__arrow">→</span>
        <code className="preview-panel__path">{destination}</code>
      </div>
      {mirror && filesToDelete > 0 && (
        <p className="preview-panel__danger">
          ⚠ Mirror mode: <strong>{filesToDelete}</strong> files or folders will be
          <strong> deleted</strong> from the destination because they no longer exist in the source.
        </p>
      )}
      {filesToCopy === 0 && filesToDelete === 0 && <p>Nothing to copy.</p>}
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
