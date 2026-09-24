import type { JobResult } from '../types'
import { formatBytes, formatDuration } from '../format'

interface Props {
  result: JobResult
  onClose: () => void
  onOpenLog: () => void
}

const OUTCOME_INFO: Record<
  JobResult['outcome'],
  { icon: string; tone: 'ok' | 'warn' | 'error'; title: string }
> = {
  success: { icon: '✓', tone: 'ok', title: 'Backup finished' },
  no_changes: { icon: '✓', tone: 'ok', title: 'Destination was already up to date' },
  success_with_mismatches: {
    icon: '!',
    tone: 'warn',
    title: 'Finished with warnings',
  },
  cancelled: { icon: '■', tone: 'warn', title: 'Backup cancelled' },
  failed: { icon: '✕', tone: 'error', title: 'Backup failed' },
  connection_error: { icon: '✕', tone: 'error', title: 'Could not connect' },
}

/** Turns robocopy's exit-code bitmask into plain language. */
function explainRobocopyExit(code: number): string {
  const bits: string[] = []
  if (code & 1) bits.push('files copied')
  if (code & 2) bits.push('extra files in the destination')
  if (code & 4) bits.push('files with attribute differences')
  if (code & 8) bits.push('some files failed')
  if (code & 16) bits.push('fatal error')
  if (bits.length === 0) return 'no changes'
  return bits.join(', ')
}

export function ResultsPanel({ result, onClose, onOpenLog }: Props) {
  const info = OUTCOME_INFO[result.outcome]
  return (
    <div className={`results-panel results-panel--${info.tone}`}>
      <div className="results-panel__icon" role="img" aria-label={info.title}>{info.icon}</div>
      <h2>{info.title}</h2>
      {result.outcome === 'success_with_mismatches' && (
        <p className="results-panel__hint">
          Some files had different attributes or permissions. The data itself was copied correctly.
          Check the log for details.
        </p>
      )}
      {result.error && <p className="results-panel__error">{result.error}</p>}
      {result.summary?.engine === 'robocopy' && (
        <table className="results-table">
          <tbody>
            <tr>
              <td>Folders copied</td>
              <td>{result.summary.dirs_copied}</td>
            </tr>
            <tr>
              <td>Files copied</td>
              <td>{result.summary.files_copied}</td>
            </tr>
            {result.summary.files_failed > 0 && (
              <tr className="results-table__bad">
                <td>Files that failed</td>
                <td>{result.summary.files_failed}</td>
              </tr>
            )}
            {result.summary.files_extra > 0 && (
              <tr>
                <td>Files only in the destination</td>
                <td>{result.summary.files_extra}</td>
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
            <tr>
              <td>Result</td>
              <td>{explainRobocopyExit(result.summary.exit_code)}</td>
            </tr>
          </tbody>
        </table>
      )}
      {result.summary?.engine === 'restic' && (
        <table className="results-table">
          <tbody>
            <tr>
              <td>New files</td>
              <td>{result.summary.files_new}</td>
            </tr>
            <tr>
              <td>Changed files</td>
              <td>{result.summary.files_changed}</td>
            </tr>
            <tr>
              <td>Unchanged</td>
              <td>{result.summary.files_unmodified}</td>
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
      <div className="results-panel__actions">
        <button type="button" onClick={onOpenLog}>
          Open logs folder
        </button>
        <button type="button" className="primary" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  )
}
