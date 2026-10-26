import type { JobPhase } from './types'

// The NAS may be the source as well as the destination (backed up to a USB
// drive, say), and a restic repository may be a local folder: the step's
// detail names the share, host or repository.
export const PHASE_LABEL: Record<JobPhase, string> = {
  checking: 'Checking the source and destination…',
  connecting: 'Connecting…',
  waking_target: 'Waking the NAS (Wake-on-LAN)…',
  scanning: 'Comparing the source with the destination…',
  copying: 'Copying…',
}

/** How a finished step reads in the list of what the job has done. */
export const PHASE_DONE_LABEL: Record<JobPhase, string> = {
  checking: 'Source and destination checked',
  connecting: 'Connected',
  waking_target: 'NAS awake',
  scanning: 'Comparison finished',
  copying: 'Copy finished',
}
