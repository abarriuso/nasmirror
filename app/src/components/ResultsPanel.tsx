import { useState } from 'react'
import { api } from '../api'
import type { JobResult, Profile } from '../types'
import { formatBytes, formatCount, formatDuration } from '../format'
import { OUTCOME } from '../outcomes'
import { JobSteps, type JobStep } from './JobSteps'

interface Props {
  result: JobResult
  /** The job that ran. In mirror mode the destination's extra files are deleted. */
  profile: Profile
  /** What the run did, step by step, for its details. */
  steps: JobStep[]
  onClose: () => void
  onOpenLogDir: () => void
}

/** The history id of a run: its log's file name without the extension. */
function logId(logPath: string): string {
  return (logPath.split(/[\\/]/).pop() ?? '').replace(/\.log$/i, '')
}

/** Turns robocopy's exit-code bitmask into plain language. */
function explainRobocopyExit(code: number, mirror: boolean): string {
  const bits: string[] = []
  if (code & 1) bits.push('files copied')
  if (code & 2) bits.push(mirror ? 'extra files deleted from the destination' : 'extra files in the destination')
  // Not attributes: a name that is a file on one side and a folder on the other.
  if (code & 4) bits.push('mismatched files or folders not copied')
  if (code & 8) bits.push('some files failed')
  if (code & 16) bits.push('fatal error')
  if (bits.length === 0) return 'no changes'
  return bits.join(', ')
}

export function ResultsPanel({ result, profile, steps, onClose, onOpenLogDir }: Props) {
  const info = OUTCOME[result.outcome]
  const mirror = profile.mode === 'mirror'
  const [log, setLog] = useState<string | null>(null)
  const toggleLog = async () => {
    if (log !== null) return setLog(null)
    try {
      setLog(await api.readLog(logId(result.log_path)))
    } catch (e: unknown) {
      setLog(e instanceof Error ? e.message : String(e))
    }
  }
  // A stopped robocopy prints no summary: the backend counts the files it saw
  // finish, but the folders and an exit code of its own are not known.
  const stopped = result.outcome === 'cancelled'
  return (
    <div className={`results-panel results-panel--${info.tone}`}>
      <div className="results-panel__icon" role="img" aria-label={info.title}>{info.icon}</div>
      <h2>{info.title}</h2>
      <p className="results-panel__job">
        <strong>{profile.name}</strong>
        <span className="results-panel__route" title={`${profile.source} → ${profile.destination}`}>
          {profile.source} → {profile.destination}
        </span>
      </p>
      {/* Robocopy's meaning of "warnings"; restic explains its own in `error`. */}
      {result.outcome === 'success_with_mismatches' && result.summary?.engine !== 'restic' && (
        <p className="results-panel__hint">
          Robocopy found a name that is a file in the source and a folder in the destination, or
          the reverse. It cannot copy one over the other, so those files are not in the copy.
          Rename or remove one of the two and run the job again.
        </p>
      )}
      {result.error && <p className="results-panel__error">{result.error}</p>}
      {result.summary?.engine === 'robocopy' && (
        <table className="results-table">
          <tbody>
            {!stopped && (
              <tr>
                <td>Folders created or updated</td>
                <td>{formatCount(result.summary.dirs_copied)}</td>
              </tr>
            )}
            <tr>
              <td>{stopped ? 'Files copied before stopping' : 'Files copied'}</td>
              <td>{formatCount(result.summary.files_copied)}</td>
            </tr>
            {result.summary.files_failed > 0 && (
              <tr className="results-table__bad">
                <td>Files that failed</td>
                <td>{formatCount(result.summary.files_failed)}</td>
              </tr>
            )}
            {result.summary.files_extra > 0 && (
              <tr>
                <td>{mirror ? 'Deleted from the destination' : 'Files only in the destination'}</td>
                <td>{formatCount(result.summary.files_extra)}</td>
              </tr>
            )}
            <tr>
              <td>Data transferred</td>
              <td>{formatBytes(result.summary.bytes_copied)}</td>
            </tr>
            <tr>
              <td>Duration</td>
              <td>{formatDuration(result.elapsed_secs)}</td>
            </tr>
            {!stopped && (
              <tr>
                <td>Result</td>
                <td>
                  {explainRobocopyExit(result.summary.exit_code, mirror)}{' '}
                  <span className="results-table__code">(exit code {result.summary.exit_code})</span>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
      {result.summary?.engine === 'restic' && (
        <table className="results-table">
          <tbody>
            <tr>
              <td>New files</td>
              <td>{formatCount(result.summary.files_new)}</td>
            </tr>
            <tr>
              <td>Changed files</td>
              <td>{formatCount(result.summary.files_changed)}</td>
            </tr>
            <tr>
              <td>Unchanged</td>
              <td>{formatCount(result.summary.files_unmodified)}</td>
            </tr>
            <tr>
              <td>New space used</td>
              <td>{formatBytes(result.summary.data_added)}</td>
            </tr>
            <tr>
              <td>Duration</td>
              <td>{formatDuration(result.elapsed_secs)}</td>
            </tr>
            <tr>
              <td>Version id</td>
              <td className="mono">{result.summary.snapshot_id.slice(0, 12)}</td>
            </tr>
          </tbody>
        </table>
      )}
      {steps.length > 0 && (
        <details className="details results-panel__details">
          <summary>Steps and commands</summary>
          <JobSteps steps={steps} scan={null} source={profile.source} />
        </details>
      )}
      {log !== null && (
        <pre className="history__log results-panel__log" aria-label="Log of this run">
          {log}
        </pre>
      )}
      <div className="results-panel__actions">
        {result.has_log && (
          <button type="button" aria-expanded={log !== null} onClick={toggleLog}>
            {log === null ? 'View log' : 'Hide log'}
          </button>
        )}
        <button type="button" onClick={onOpenLogDir}>
          Open logs folder
        </button>
        <button type="button" className="primary" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  )
}
