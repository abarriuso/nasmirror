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
 * Whether `mac` reads as a MAC address the backend accepts: 12 hex digits,
 * with any separators (`AA:BB:..`, `aa-bb-..`, `aabb..`).
 */
export function isValidMac(mac: string): boolean {
  return /^[0-9a-f]{12}$/i.test(mac.replace(/[^0-9a-z]/gi, ''))
}

/**
 * When a run finished, from the history's "2026-10-23 20:09:07": "today
 * 20:09", "yesterday 20:09", or "23 Oct 20:09" (with the year when it is
 * not this one). `today` is for the tests.
 */
export function formatWhen(stamp: string, today = new Date()): string {
  const m = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2})/.exec(stamp)
  if (!m) return stamp
  const [, y, mo, d, h, mi] = m
  const day = new Date(Number(y), Number(mo) - 1, Number(d))
  const midnight = new Date(today.getFullYear(), today.getMonth(), today.getDate())
  const daysAgo = Math.round((midnight.getTime() - day.getTime()) / 86_400_000)
  const time = `${h}:${mi}`
  if (daysAgo === 0) return `today ${time}`
  if (daysAgo === 1) return `yesterday ${time}`
  const month = day.toLocaleString('en-GB', { month: 'short' })
  const year = day.getFullYear() === today.getFullYear() ? '' : ` ${y}`
  return `${Number(d)} ${month}${year} ${time}`
}

/** A count with thousands separators, in the UI's English: 12,345. */
export function formatCount(n: number): string {
  return Math.round(n).toLocaleString('en-US')
}

/** "1 file", "2 files", "12,345 files". */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`
}

/** A short duration for a step or a wait: "0.4 s", "42 s", "3 min 05 s", "1 h 02 min". */
export function formatElapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) ms = 0
  const s = ms / 1000
  if (s < 10) return `${s.toFixed(1)} s`
  if (s < 60) return `${Math.floor(s)} s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m} min ${String(Math.floor(s % 60)).padStart(2, '0')} s`
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')} min`
}

/**
 * `path` shown from `base` on (`…\2026\trip`), or whole when it is not
 * inside `base`: the shared start of every path in a scan only takes room.
 */
export function relativePath(base: string, path: string): string {
  const b = base.replace(/[\\/]+$/, '')
  if (b && path.toLowerCase().startsWith(`${b.toLowerCase()}\\`)) {
    const rest = path.slice(b.length + 1).replace(/\\$/, '')
    return rest ? `…\\${rest}` : path
  }
  return path
}

/**
 * `D:\folder` or `\\host\share\folder`, as the backend requires: anything
 * else (`\nas\share`, `backup`) Windows resolves against a folder nobody
 * typed.
 */
export function isFullPath(path: string): boolean {
  const p = path.trim()
  return /^[a-z]:[\\/]/i.test(p) || /^(\\\\|\/\/)[^\\/]+[\\/][^\\/]+/.test(p)
}

/**
 * The root of a drive (`D:\`) or of a network share (`\\nas\share`):
 * mirroring into one deletes everything else stored there.
 */
export function isRootPath(path: string): boolean {
  const p = path.trim().replace(/\//g, '\\').replace(/\\+$/, '')
  return /^[a-z]:$/i.test(p) || /^\\\\[^\\]+\\[^\\]+$/.test(p)
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
  /** Mirror mode's deletions, which take time and move no bytes. */
  deletedDone = 0,
  deleteTotal = 0,
): number {
  const parts: number[] = []
  if (bytesTotal > 0) parts.push(Math.min(1, bytesDone / bytesTotal))
  if (filesTotal > 0) parts.push(Math.min(1, filesDone / filesTotal))
  if (deleteTotal > 0) parts.push(Math.min(1, deletedDone / deleteTotal))
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
