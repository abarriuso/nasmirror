export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) bytes = 0
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  const decimals = i === 0 ? 0 : v < 10 ? 2 : 1
  return `${v.toFixed(decimals)} ${units[i]}`
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`
}

export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) totalSeconds = 0
  const h = Math.floor(totalSeconds / 3600)
  const m = Math.floor((totalSeconds % 3600) / 60)
  const s = Math.floor(totalSeconds % 60)
  return [h, m, s].map((n) => String(n).padStart(2, '0')).join(':')
}

export function formatEtaRange(secondsLow: number, secondsHigh: number): string {
  if (secondsLow < 30 && secondsHigh < 30) return 'less than 30 s'
  const fmt = (s: number) => {
    const m = Math.round(s / 60)
    if (m < 1) return '<1 min'
    if (m < 60) return `${m} min`
    const h = Math.floor(m / 60)
    const rm = m % 60
    return rm ? `${h} h ${rm} min` : `${h} h`
  }
  const lo = fmt(secondsLow)
  const hi = fmt(secondsHigh)
  return lo === hi ? lo : `${lo}–${hi}`
}

/**
 * Splits a list typed as entries separated by spaces. An entry that contains
 * spaces goes in double quotes: `.git "System Volume Information"`.
 */
export function parseList(text: string): string[] {
  const items: string[] = []
  for (const m of text.matchAll(/"([^"]*)"?|(\S+)/g)) {
    const item = (m[1] ?? m[2]).trim()
    if (item) items.push(item)
  }
  return items
}

/** The text `parseList` reads back as `items`. */
export function formatList(items: string[]): string {
  return items.map((item) => (/\s/.test(item) ? `"${item}"` : item)).join(' ')
}

/**
 * How far a copy has got, from 0 to 1. Bytes alone mislead while robocopy
 * works through many small files (each takes time but moves few bytes), and
 * files alone while it copies a few large ones: the average of both is a
 * steadier basis for the time left.
 */
export function copyProgress(
  bytesDone: number,
  bytesTotal: number,
  filesDone: number,
  filesTotal: number,
): number {
  const parts: number[] = []
  if (bytesTotal > 0) parts.push(Math.min(1, bytesDone / bytesTotal))
  if (filesTotal > 0) parts.push(Math.min(1, filesDone / filesTotal))
  return parts.length ? parts.reduce((a, b) => a + b, 0) / parts.length : 0
}

/**
 * Seconds left at the pace the samples of the last `windowMs` show
 * (`progress` as from `copyProgress`), or 0 while there is no pace to go by.
 */
export function secondsLeft(samples: { tMs: number; progress: number }[], windowMs: number): number {
  if (samples.length < 2) return 0
  const last = samples[samples.length - 1]
  const first = samples.find((s) => s.tMs >= last.tMs - windowMs) ?? samples[0]
  const gained = last.progress - first.progress
  const seconds = (last.tMs - first.tMs) / 1000
  if (gained <= 0 || seconds <= 0) return 0
  return ((1 - last.progress) * seconds) / gained
}
