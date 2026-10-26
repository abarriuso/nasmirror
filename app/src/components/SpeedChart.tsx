import { memo, useEffect, useLayoutEffect, useRef } from 'react'

export interface SpeedSample {
  /** Date.now() when the sample reached the frontend (not the backend time). */
  tMs: number
  bytesPerSec: number
  /**
   * Files copied plus things deleted per second: deletions and small files
   * keep robocopy busy while moving few bytes, and the bytes line alone
   * then lay flat as if nothing happened.
   */
  itemsPerSec: number
  /** How far the copy had got, from 0 to 1 (see `copyProgress`). */
  progress: number
}

type Series = 'bytesPerSec' | 'itemsPerSec'

interface Props {
  samples: SpeedSample[]
}

const WINDOW_MS = 60_000
const HEIGHT_PX = 160

/**
 * Live speed chart. Interpolates linearly between real samples inside a
 * requestAnimationFrame loop, so it moves at the monitor's refresh rate
 * instead of jumping every 200 ms.
 */
export const SpeedChart = memo(function SpeedChart({ samples }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  // The rAF loop always reads the latest samples without restarting on every render.
  const samplesRef = useRef(samples)
  useLayoutEffect(() => {
    samplesRef.current = samples
  }, [samples])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let raf = 0
    const dpr = Math.max(1, window.devicePixelRatio || 1)

    const resize = () => {
      const rect = canvas.getBoundingClientRect()
      canvas.width = Math.round(rect.width * dpr)
      canvas.height = Math.round(rect.height * dpr)
    }
    const ro = new ResizeObserver(resize)
    ro.observe(canvas)
    resize()

    const interpolatedAt = (now: number, series: Series = 'bytesPerSec'): number => {
      const s = samplesRef.current
      if (s.length === 0) return 0
      if (s.length === 1) return s[0][series]
      let i = s.length - 1
      while (i > 0 && s[i].tMs > now) i--
      const a = s[i]
      const b = s[Math.min(i + 1, s.length - 1)]
      if (a === b || b.tMs === a.tMs) return a[series]
      const t = Math.min(1, Math.max(0, (now - a.tMs) / (b.tMs - a.tMs)))
      return a[series] + (b[series] - a[series]) * t
    }
    const interpolatedSpeedAt = (now: number) => interpolatedAt(now)

    const draw = () => {
      raf = requestAnimationFrame(draw)

      const w = canvas.width
      const h = canvas.height
      ctx.clearRect(0, 0, w, h)

      const wallNow = Date.now()
      const windowStart = wallNow - WINDOW_MS
      // Find the first visible sample instead of filtering into a new array every frame
      const s = samplesRef.current
      let startIdx = 0
      for (let i = 0; i < s.length; i++) {
        if (s[i].tMs >= windowStart - 1000) { startIdx = i; break }
      }
      const visible = s.slice(startIdx)
      // Compute the max with a loop: spreading a large array into Math.max can overflow the stack
      let maxSpeed = 0
      for (let i = 0; i < visible.length; i++) {
        if (visible[i].bytesPerSec > maxSpeed) maxSpeed = visible[i].bytesPerSec
      }
      if (maxSpeed < 1024 * 1024) maxSpeed = 1024 * 1024

      // Subtle grid
      ctx.strokeStyle = 'rgba(148, 163, 184, 0.12)'
      ctx.lineWidth = 1
      for (let i = 1; i < 4; i++) {
        const y = (h / 4) * i
        ctx.beginPath()
        ctx.moveTo(0, y)
        ctx.lineTo(w, y)
        ctx.stroke()
      }

      if (visible.length >= 1) {
        // Scale the interpolation step to the canvas's real width
        const cssWidth = w / dpr
        const stepMs = Math.max(16, Math.ceil(WINDOW_MS / Math.max(cssWidth, 200)))
        ctx.beginPath()
        let first = true
        for (let t = windowStart; t <= wallNow; t += stepMs) {
          const x = ((t - windowStart) / WINDOW_MS) * w
          const speed = interpolatedSpeedAt(t)
          const y = h - (speed / maxSpeed) * h
          if (first) {
            ctx.moveTo(x, y)
            first = false
          } else {
            ctx.lineTo(x, y)
          }
        }
        const nowSpeed = interpolatedSpeedAt(wallNow)
        const nowY = h - (nowSpeed / maxSpeed) * h
        ctx.lineTo(w, nowY)

        const grad = ctx.createLinearGradient(0, 0, 0, h)
        grad.addColorStop(0, 'rgba(56, 189, 248, 0.85)')
        grad.addColorStop(1, 'rgba(56, 189, 248, 0.15)')
        ctx.strokeStyle = grad
        ctx.lineWidth = 2 * dpr
        ctx.lineJoin = 'round'
        ctx.stroke()

        ctx.lineTo(w, h)
        ctx.lineTo(0, h)
        ctx.closePath()
        ctx.fillStyle = 'rgba(56, 189, 248, 0.12)'
        ctx.fill()

        ctx.beginPath()
        ctx.arc(w, nowY, 3 * dpr, 0, Math.PI * 2)
        ctx.fillStyle = '#38bdf8'
        ctx.fill()

        // Files per second, on a scale of its own (at least 10/s).
        let maxItems = 10
        for (let i = 0; i < visible.length; i++) {
          if (visible[i].itemsPerSec > maxItems) maxItems = visible[i].itemsPerSec
        }
        ctx.beginPath()
        first = true
        for (let t = windowStart; t <= wallNow; t += stepMs) {
          const x = ((t - windowStart) / WINDOW_MS) * w
          const y = h - (interpolatedAt(t, 'itemsPerSec') / maxItems) * h * 0.95
          if (first) {
            ctx.moveTo(x, y)
            first = false
          } else {
            ctx.lineTo(x, y)
          }
        }
        ctx.lineTo(w, h - (interpolatedAt(wallNow, 'itemsPerSec') / maxItems) * h * 0.95)
        ctx.strokeStyle = 'rgba(52, 211, 153, 0.85)'
        ctx.lineWidth = 1.5 * dpr
        ctx.stroke()
      }
    }

    raf = requestAnimationFrame(draw)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  }, [])

  return (
    <div className="speed-chart">
      <canvas
        ref={canvasRef}
        style={{ width: '100%', height: HEIGHT_PX, display: 'block' }}
        role="img"
        aria-label="Transfer speed and files per second, last minute"
      />
      <div className="speed-chart__legend" aria-hidden="true">
        <span className="speed-chart__key speed-chart__key--bytes">Data per second</span>
        <span className="speed-chart__key speed-chart__key--items">Files copied or deleted per second</span>
      </div>
    </div>
  )
})
