import { memo } from 'react'
import type { JobPhase } from '../types'
import { PHASE_LABEL } from '../phases'
import { SpeedChart, type SpeedSample } from './SpeedChart'
import { formatBytes, formatEtaRange, formatSpeed, secondsLeft } from '../format'

interface Props {
  phase: JobPhase
  bytesDone: number
  bytesTotal: number
  filesDone: number
  filesTotal: number
  currentFile: string
  samples: SpeedSample[]
  onCancel: () => void
}

const ETA_AFTER_MS = 10_000

/** Average speed over the samples from the last `windowMs`. */
function recentAvgSpeed(samples: SpeedSample[], windowMs: number): number {
  if (samples.length === 0) return 0
  const cutoff = Date.now() - windowMs
  const recent = samples.filter((s) => s.tMs >= cutoff)
  const pool = recent.length >= 2 ? recent : samples.slice(-2)
  if (pool.length === 0) return 0
  return pool.reduce((a, b) => a + b.bytesPerSec, 0) / pool.length
}

export const JobView = memo(function JobView({
  phase,
  bytesDone,
  bytesTotal,
  filesDone,
  filesTotal,
  currentFile,
  samples,
  onCancel,
}: Props) {
  const pct = bytesTotal > 0 ? Math.min(100, (bytesDone / bytesTotal) * 100) : 0
  const speed10s = recentAvgSpeed(samples, 10_000)
  // From the pace of files and bytes together, over the last 10 s and 60 s:
  // estimated from bytes alone, a run of small files once read "313 h left".
  const etas = [secondsLeft(samples, 10_000), secondsLeft(samples, 60_000)].filter((s) => s > 0)
  // The first seconds are still no basis for an estimate.
  const copyingForMs = samples.length > 1 ? samples[samples.length - 1].tMs - samples[0].tMs : 0
  const showEta = phase === 'copying' && copyingForMs >= ETA_AFTER_MS && etas.length > 0

  return (
    <div className="job-view">
      <div className="job-view__hero">
        <div className="job-view__pct">{phase === 'copying' ? `${pct.toFixed(0)}%` : PHASE_LABEL[phase]}</div>
        <div className="job-view__meta">
          {phase === 'copying' && <div className="job-view__phase">{PHASE_LABEL[phase]}</div>}
          {showEta && (
            <div className="job-view__eta">{formatEtaRange(Math.min(...etas), Math.max(...etas))} left</div>
          )}
        </div>
      </div>

      <div
        className="progress-track"
        role="progressbar"
        aria-valuenow={Math.round(pct)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Copy progress"
      >
        <div
          className="progress-fill"
          style={{ width: `${phase === 'copying' || phase === 'done' ? pct : 0}%` }}
        />
      </div>
      {currentFile && <div className="job-view__current-file" title={currentFile}>{currentFile}</div>}

      <div className="job-view__stats" aria-live="polite">
        <Stat label="Transferred" value={formatBytes(bytesDone)} />
        <Stat label="Total" value={formatBytes(bytesTotal)} />
        <Stat label="Current speed" value={formatSpeed(speed10s)} />
        <Stat label="Files" value={`${filesDone} / ${filesTotal}`} />
      </div>

      <SpeedChart samples={samples} />

      <div className="job-view__actions">
        <button type="button" className="danger" onClick={onCancel}>
          Stop
        </button>
      </div>
    </div>
  )
})

const Stat = memo(function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat-tile">
      <span className="stat-tile__label">{label}</span>
      <span className="stat-tile__value">{value}</span>
    </div>
  )
})
