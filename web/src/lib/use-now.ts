import { useEffect, useState } from 'react'

const UNIT_MS = { second: 1_000, minute: 60_000 } as const

/**
 * Current time, re-rendering at each second/minute boundary (aligned to the
 * clock, so a minute display flips exactly when the minute changes).
 */
export function useNow(granularity: keyof typeof UNIT_MS = 'minute'): Date {
  const [now, setNow] = useState(() => new Date())

  useEffect(() => {
    const unit = UNIT_MS[granularity]
    let timer: ReturnType<typeof setTimeout> | undefined
    const schedule = () => {
      const wait = unit - (Date.now() % unit) + 20
      timer = setTimeout(() => {
        setNow(new Date())
        schedule()
      }, wait)
    }
    schedule()
    return () => {
      clearTimeout(timer)
    }
  }, [granularity])

  return now
}
