import type { JobPhase, ScanTotals } from '../types'
import { useNow } from '../useNow'
import { PHASE_DONE_LABEL, PHASE_LABEL } from '../phases'
import { formatBytes, formatElapsed, plural, relativePath } from '../format'

/** One thing the job did or is doing, as the backend reported it. */
export interface JobStep {
  phase: JobPhase
  /** The paths, the share and user, or the command line of the step. */
  detail: string | null
  startedMs: number
  /** `null` while it is the step in progress. */
  endedMs: number | null
  /** How it ended: the job's last step takes the job's outcome. */
  ended?: StepEnd
}

export type StepEnd = 'done' | 'failed' | 'stopped'

const ENDED_ICON: Record<StepEnd, string> = { done: '✓', failed: '✕', stopped: '■' }

/** How far the scan has got (the backend reports it a few times a second). */
export interface ScanSnapshot {
  dirsScanned: number
  currentDir: string
  totals: ScanTotals
}

interface Props {
  steps: JobStep[]
  scan: ScanSnapshot | null
  /** The job's source, to show the scan's current folder from there on. */
  source: string
}

/**
 * The steps of a job so far, each with how long it took and what exactly it
 * does: a scan of a large tree over the network can take minutes, and a bare
 * "Working out what to copy…" looked stuck.
 */
export function JobSteps({ steps, scan, source }: Props) {
  const running = steps.some((s) => s.endedMs === null)
  const now = useNow(running)
  if (steps.length === 0) return null
  return (
    <ol className="job-steps" aria-label="What the job is doing">
      {steps.map((step, i) => {
        const active = step.endedMs === null
        const ended = step.ended ?? 'done'
        return (
          <li key={i} className={`job-step job-step--${active ? 'active' : ended}`}>
            <span className="job-step__icon" aria-hidden="true">
              {active ? <span className="spinner" /> : ENDED_ICON[ended]}
            </span>
            <div className="job-step__body">
              <div className="job-step__line">
                <span className="job-step__label">{stepLabel(step.phase, active, ended)}</span>
                <span className="job-step__time">
                  {formatElapsed((step.endedMs ?? now) - step.startedMs)}
                </span>
              </div>
              {step.phase === 'scanning' && scan && (
                <ScanStats scan={scan} source={source} active={active} />
              )}
              {step.detail && (
                <code className="job-step__detail" title={step.detail}>
                  {step.detail}
                </code>
              )}
            </div>
          </li>
        )
      })}
    </ol>
  )
}

function ScanStats({ scan, source, active }: { scan: ScanSnapshot; source: string; active: boolean }) {
  const { totals } = scan
  return (
    <div className="job-step__stats" aria-live="polite">
      <span>
        {plural(scan.dirsScanned, 'folder')} checked · {plural(totals.files_to_copy, 'file')} to
        copy ({formatBytes(totals.bytes_to_copy)})
        {totals.files_to_delete > 0 && <> · {formatDeleted(totals.files_to_delete)}</>}
        {!active && totals.dirs_to_create > 0 && <> · {plural(totals.dirs_to_create, 'new folder')}</>}
      </span>
      {active && scan.currentDir && (
        <span className="job-step__current" title={scan.currentDir}>
          In {relativePath(source, scan.currentDir)}
        </span>
      )}
    </div>
  )
}

function stepLabel(phase: JobPhase, active: boolean, ended: StepEnd): string {
  if (active) return PHASE_LABEL[phase]
  if (ended === 'done') return PHASE_DONE_LABEL[phase]
  // "Connecting to the destination…" → "Connecting to the destination failed"
  return `${PHASE_LABEL[phase].replace(/…$/, '')} ${ended === 'failed' ? 'failed' : 'stopped'}`
}

function formatDeleted(n: number): string {
  return `${plural(n, 'file or folder', 'files or folders')} to delete`
}
