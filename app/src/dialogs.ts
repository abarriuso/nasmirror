import { confirm, message } from '@tauri-apps/plugin-dialog'

// `window.confirm` cannot be used: the dialog plugin replaces it with an async
// function that calls a command the plugin no longer has, so it never shows a
// dialog and its promise always reads as "yes".

/** Asks before doing something that cannot be undone. Anything but an explicit OK is a no. */
export function askToConfirm(text: string, okLabel: string): Promise<boolean> {
  return confirm(text, { title: 'NASMirror', kind: 'warning', okLabel, cancelLabel: 'Cancel' }).catch(
    () => false,
  )
}

export function showError(text: string): void {
  message(text, { title: 'NASMirror', kind: 'error' }).catch(() => {})
}
