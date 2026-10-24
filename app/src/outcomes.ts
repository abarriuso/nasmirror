import type { Outcome } from './types'

/** How a run ended, as a short badge: the history list and each job card. */
export const OUTCOME_BADGE: Record<Outcome, { label: string; tone: 'ok' | 'warn' | 'error' }> = {
  success: { label: 'Finished', tone: 'ok' },
  no_changes: { label: 'No changes', tone: 'ok' },
  success_with_mismatches: { label: 'With warnings', tone: 'warn' },
  cancelled: { label: 'Cancelled', tone: 'warn' },
  failed: { label: 'Failed', tone: 'error' },
  connection_error: { label: 'No connection', tone: 'error' },
}
