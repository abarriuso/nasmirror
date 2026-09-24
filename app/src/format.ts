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
