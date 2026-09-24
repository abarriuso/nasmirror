import type { JobPhase } from './types'

export const PHASE_LABEL: Record<JobPhase, string> = {
  connecting: 'Connecting to the destination…',
  waking_target: 'Waking the destination (Wake-on-LAN)…',
  scanning: 'Working out what to copy…',
  copying: 'Copying…',
  cancelling: 'Cancelling…',
  done: 'Done',
}
