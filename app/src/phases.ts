import type { JobPhase } from './types'

export const PHASE_LABEL: Record<JobPhase, string> = {
  checking: 'Checking the source and destination…',
  connecting: 'Connecting to the destination…',
  waking_target: 'Waking the destination (Wake-on-LAN)…',
  scanning: 'Comparing the source with the destination…',
  copying: 'Copying…',
  cancelling: 'Cancelling…',
  done: 'Done',
}

/** How a finished step reads in the list of what the job has done. */
export const PHASE_DONE_LABEL: Record<JobPhase, string> = {
  checking: 'Source and destination checked',
  connecting: 'Connected to the destination',
  waking_target: 'Destination awake',
  scanning: 'Comparison finished',
  copying: 'Copy finished',
  cancelling: 'Cancelled',
  done: 'Done',
}
