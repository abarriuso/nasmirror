import type { Outcome } from './types'

/**
 * How a run ended, said once for every place that says it: the result
 * screen and the system notification (`title`, `icon`), and the short badge
 * on the history list and each job card (`label`).
 */
export const OUTCOME: Record<
  Outcome,
  { title: string; label: string; icon: string; tone: 'ok' | 'warn' | 'error' }
> = {
  success: { title: 'Backup finished', label: 'Finished', icon: '✓', tone: 'ok' },
  no_changes: {
    title: 'Destination was already up to date',
    label: 'No changes',
    icon: '✓',
    tone: 'ok',
  },
  success_with_mismatches: {
    title: 'Finished with warnings',
    label: 'With warnings',
    icon: '!',
    tone: 'warn',
  },
  cancelled: { title: 'Backup cancelled', label: 'Cancelled', icon: '■', tone: 'warn' },
  failed: { title: 'Backup failed', label: 'Failed', icon: '✕', tone: 'error' },
  connection_error: { title: 'Could not connect', label: 'No connection', icon: '✕', tone: 'error' },
}
