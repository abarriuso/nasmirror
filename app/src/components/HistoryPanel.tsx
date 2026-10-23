import { useEffect, useState } from 'react'
import { api } from '../api'
import { askToConfirm } from '../dialogs'
import { formatBytes, formatDuration, formatWhen } from '../format'
import type { HistoryEntry } from '../types'
import { OUTCOME_BADGE } from '../outcomes'

interface Props {
  onClose: () => void
}

export function HistoryPanel({ onClose }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [detail, setDetail] = useState<{ entry: HistoryEntry; log: string } | null>(null)

  useEffect(() => {
    api
      .listHistory()
      .then(setEntries)
      .catch((e: unknown) => {
        setEntries([])
        setError(e instanceof Error ? e.message : String(e))
      })
  }, [])

  const openLog = async (entry: HistoryEntry) => {
    try {
      setDetail({ entry, log: await api.readLog(entry.id) })
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const remove = async (entry: HistoryEntry) => {
    if (!(await askToConfirm('Delete this history entry? It does not affect the copied files.', 'Delete')))
      return
    try {
      setEntries(await api.deleteHistoryEntry(entry.id))
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  if (detail) {
    return (
      <div className="history">
        <div className="history__toolbar">
          <h1>
            {detail.entry.profile_name || 'Earlier backup'} · {formatWhen(detail.entry.finished_at)}
          </h1>
          <button type="button" onClick={() => setDetail(null)}>
            Back to history
          </button>
        </div>
        <pre className="history__log">{detail.log}</pre>
      </div>
    )
  }

  return (
    <div className="history">
      <div className="history__toolbar">
        <h1>History</h1>
        <div className="history__toolbar-actions">
          <button type="button" onClick={() => api.openLogDir()}>
            Open folder
          </button>
          <button type="button" className="primary" onClick={onClose}>
            Back
          </button>
        </div>
      </div>

      {error && <p className="history__error">{error}</p>}

      {entries === null && (
        <div aria-busy="true" aria-live="polite">
          <span className="sr-only">Loading history…</span>
          <div className="skeleton-card" />
          <div className="skeleton-card" />
        </div>
      )}

      {entries?.length === 0 && (
        <p className="profile-list__empty">
          No backups recorded yet. Every run will show up here with its result.
        </p>
      )}

      <ul className="history__list" role="list">
        {entries?.map((entry) => {
          const badge = entry.outcome ? OUTCOME_BADGE[entry.outcome] : null
          return (
            <li key={entry.id} className="history__item">
              <div className="history__main">
                <div className="history__line">
                  <strong>{entry.profile_name || 'Earlier backup'}</strong>
                  {badge ? (
                    <span className={`history__badge history__badge--${badge.tone}`}>
                      {badge.label}
                    </span>
                  ) : (
                    // Logs from versions before run history: only the log text
                    // is kept, not how the run ended.
                    <span className="history__badge">No details</span>
                  )}
                </div>
                <span className="history__meta" title={entry.finished_at}>
                  {formatWhen(entry.finished_at)}
                </span>
                {entry.outcome && (
                  <span className="history__meta">
                    {entry.files_copied} files · {formatBytes(entry.bytes_copied)} ·{' '}
                    {formatDuration(entry.elapsed_secs)}
                  </span>
                )}
                {entry.destination && (
                  <span className="history__meta history__meta--path">{entry.destination}</span>
                )}
                {entry.error && <span className="history__meta-error">{entry.error}</span>}
              </div>
              <div className="history__actions">
                {/* A job that fails before copying (connection, WoL) writes no
                    log, so there is nothing to open. */}
                {entry.has_log && (
                  <button type="button" onClick={() => openLog(entry)}>
                    View log
                  </button>
                )}
                <button type="button" className="danger" onClick={() => remove(entry)}>
                  Delete
                </button>
              </div>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
