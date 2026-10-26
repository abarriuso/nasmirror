import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
import { formatBytes, formatDuration } from './format'
import { OUTCOME } from './outcomes'
import type { JobResult } from './types'

function body(profileName: string, result: JobResult): string {
  if (result.error) return `${profileName}: ${result.error}`
  const bytes =
    result.summary?.engine === 'robocopy'
      ? result.summary.bytes_copied
      : result.summary?.data_added
  const parts = [profileName]
  if (bytes) parts.push(formatBytes(bytes))
  parts.push(formatDuration(result.elapsed_secs))
  return parts.join(' · ')
}

/**
 * Shows a system notification when a copy ends, but only if the window is not
 * focused: when it is, the results panel already tells the user.
 */
export async function notifyFinished(profileName: string, result: JobResult): Promise<void> {
  if (document.hasFocus()) return
  try {
    let granted = await isPermissionGranted()
    if (!granted) granted = (await requestPermission()) === 'granted'
    if (!granted) return
    sendNotification({ title: OUTCOME[result.outcome].title, body: body(profileName, result) })
  } catch {
    // A notification that cannot be shown (permission denied, notifications
    // turned off in Windows) must not break the end of the copy.
  }
}
