import { relaunch } from '@tauri-apps/plugin-process'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { askToConfirm, showError } from './dialogs'

/**
 * Looks for a newer published release on GitHub. Being offline, no release
 * with an update manifest yet or any other failure reads as "no update": the
 * app works the same without it.
 */
export async function findUpdate(): Promise<Update | null> {
  // `tauri dev` runs from the source tree: installing a release from there
  // would replace the installed copy, not the one being developed.
  if (import.meta.env.DEV) return null
  try {
    return await check()
  } catch (e) {
    console.warn('Could not check for updates:', e)
    return null
  }
}

/**
 * Asks whether to install `update` now and, on OK, downloads it, installs it
 * and restarts NASMirror. On Windows the installer itself closes the app and
 * opens the new version, so `relaunch` is only reached elsewhere.
 */
export async function offerUpdate(update: Update): Promise<void> {
  const ok = await askToConfirm(
    `NASMirror ${update.version} is available (you have ${update.currentVersion}). ` +
      'Install it now? NASMirror will close and open again.',
    'Update',
  )
  if (!ok) return
  try {
    await update.downloadAndInstall()
    await relaunch()
  } catch (e) {
    // The user asked for it, so a failed download or install is not silent.
    console.warn('Could not install the update:', e)
    showError(`Could not install the update: ${e instanceof Error ? e.message : String(e)}`)
  }
}
