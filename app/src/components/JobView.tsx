import { memo } from 'react'
import { useNow } from '../useNow'
import type { JobPhase } from '../types'
import { PHASE_LABEL } from '../phases'
import { SpeedChart, type SpeedSample } from './SpeedChart'
import { JobSteps, type JobStep } from './JobSteps'
import {
  copyProgress,
  formatBytes,
  formatCount,
  formatElapsed,
  formatEtaRange,
  formatSpeed,
  relativePath,
  secondsLeft,
} from '../format'

interface Props {
  phase: JobPhase
  bytesDone: number
  bytesTotal: number
  filesDone: number
  filesTotal: number
  /** Mirror mode: deleted from the destination so far, and how many the scan found. */
  itemsDeleted: number
  bytesDeleted: number
  itemsToDelete: number
  currentFile: string
  /** `currentFile` was deleted from the destination, not copied from the source. */
  currentDeleted: boolean
  samples: SpeedSample[]
  /** What the job has done so far (connecting, copying...), with details. */
  steps: JobStep[]
  /** The job's source, to show the latest file from there on. */
  source: string
  /** Stop was confirmed and robocopy is being ended. */
  stopping: boolean
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
  itemsDeleted,
  bytesDeleted,
  itemsToDelete,
  currentFile,
  currentDeleted,
  samples,
  steps,
  source,
  stopping,
  onCancel,
}: Props) {
  const startedMs = steps[0]?.startedMs
  const now = useNow(true, 1000)
  // Bytes, files and deletions together: a run that is deleting, or copying
  // small files, moves few bytes but is still getting on.
  const pct =
    copyProgress(bytesDone, bytesTotal, filesDone, filesTotal, itemsDeleted, itemsToDelete) * 100
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
          {phase === 'copying' && (
            <div className="job-view__phase">
              {currentFile && currentDeleted ? 'Deleting what is not in the source…' : PHASE_LABEL[phase]}
            </div>
          )}
          <div className="job-view__eta">
            {startedMs !== undefined && <>{formatElapsed(now - startedMs)} elapsed</>}
            {showEta && <> · {formatEtaRange(Math.min(...etas), Math.max(...etas))} left</>}
          </div>
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
          style={{ width: `${phase === 'copying' ? pct : 0}%` }}
        />
      </div>
      {/* Robocopy names a file as it finishes it (or, with /MT, as it
          starts): "latest" fits both. */}
      {currentFile && (
        <div className="job-view__current-file" title={currentFile}>
          {currentDeleted ? 'Deleted: ' : 'Latest: '}
          {currentDeleted ? currentFile : relativePath(source, currentFile)}
        </div>
      )}

      <div className="job-view__stats" aria-live="polite">
        <Stat label="Transferred" value={formatBytes(bytesDone)} />
        <Stat label="Total" value={formatBytes(bytesTotal)} />
        <Stat label="Current speed" value={formatSpeed(speed10s)} />
        <Stat label="Files" value={`${formatCount(filesDone)} / ${formatCount(filesTotal)}`} />
        {itemsToDelete > 0 && (
          <Stat
            label={`Deleted (${formatBytes(bytesDeleted)})`}
            value={`${formatCount(itemsDeleted)} / ${formatCount(itemsToDelete)}`}
          />
        )}
      </div>

      <SpeedChart samples={samples} />

      <JobSteps steps={steps} scan={null} source={source} />

      <div className="job-view__actions">
        <button type="button" className="danger" disabled={stopping} onClick={onCancel}>
          {stopping ? 'Stopping…' : 'Stop'}
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
