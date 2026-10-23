import { useEffect, useState } from 'react'

/** The time, updated every `ms` while `active`: for clocks of something in progress. */
export function useNow(active: boolean, ms = 500): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setNow(Date.now()), ms)
    return () => window.clearInterval(id)
  }, [active, ms])
  return now
}
