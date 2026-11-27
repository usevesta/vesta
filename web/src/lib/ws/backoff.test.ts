import { describe, expect, it } from 'vitest'

import {
  backoffBaseDelay,
  backoffDelay,
  DEFAULT_BACKOFF,
  resolveBackoffOptions,
} from '@/lib/ws/backoff'

describe('backoffBaseDelay', () => {
  it('doubles from 500 ms and caps at 30 s by default', () => {
    const schedule = Array.from({ length: 10 }, (_, attempt) => backoffBaseDelay(attempt))
    expect(schedule).toEqual([
      500, 1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000, 30_000,
    ])
  })

  it('never exceeds the cap, even for huge attempts', () => {
    expect(backoffBaseDelay(10_000)).toBe(DEFAULT_BACKOFF.maxDelayMs)
    expect(backoffBaseDelay(Number.MAX_SAFE_INTEGER)).toBe(DEFAULT_BACKOFF.maxDelayMs)
  })

  it('is monotonic non-decreasing', () => {
    let previous = 0
    for (let attempt = 0; attempt < 50; attempt += 1) {
      const delay = backoffBaseDelay(attempt)
      expect(delay).toBeGreaterThanOrEqual(previous)
      previous = delay
    }
  })

  it('treats negative, fractional and non-finite attempts safely', () => {
    expect(backoffBaseDelay(-3)).toBe(500)
    expect(backoffBaseDelay(Number.NaN)).toBe(500)
    expect(backoffBaseDelay(2.9)).toBe(2_000)
  })

  it('honours custom options', () => {
    const options = { initialDelayMs: 100, multiplier: 3, maxDelayMs: 1_000 }
    expect([0, 1, 2, 3].map((n) => backoffBaseDelay(n, options))).toEqual([100, 300, 900, 1_000])
  })
})

describe('backoffDelay (with jitter)', () => {
  it('equals the base delay when random() returns 0', () => {
    expect(backoffDelay(3, {}, () => 0)).toBe(4_000)
  })

  it('removes at most `jitter` of the base delay', () => {
    // r -> 1 gives the smallest delay: base * (1 - 0.3)
    expect(backoffDelay(3, {}, () => 0.999_999)).toBe(2_800)
    expect(backoffDelay(20, {}, () => 0.999_999)).toBe(21_000)
  })

  it('stays within (base * (1 - jitter), base] for random values', () => {
    let seed = 42
    const random = () => {
      // Small deterministic LCG so the test is reproducible.
      seed = (seed * 1_664_525 + 1_013_904_223) % 2 ** 32
      return seed / 2 ** 32
    }
    for (let attempt = 0; attempt < 30; attempt += 1) {
      const base = backoffBaseDelay(attempt)
      for (let i = 0; i < 20; i += 1) {
        const delay = backoffDelay(attempt, {}, random)
        expect(delay).toBeLessThanOrEqual(base)
        expect(delay).toBeGreaterThanOrEqual(Math.round(base * (1 - DEFAULT_BACKOFF.jitter)))
        expect(delay).toBeLessThanOrEqual(30_000)
      }
    }
  })

  it('has no randomness when jitter is 0', () => {
    expect(backoffDelay(4, { jitter: 0 }, () => 0.7)).toBe(8_000)
  })

  it('clamps out-of-range random values', () => {
    expect(backoffDelay(0, {}, () => -5)).toBe(500)
    expect(backoffDelay(0, {}, () => 5)).toBe(350)
  })
})

describe('resolveBackoffOptions', () => {
  it('rejects invalid options', () => {
    expect(() => resolveBackoffOptions({ initialDelayMs: -1 })).toThrow(RangeError)
    expect(() => resolveBackoffOptions({ maxDelayMs: 100 })).toThrow(RangeError)
    expect(() => resolveBackoffOptions({ multiplier: 0.5 })).toThrow(RangeError)
    expect(() => resolveBackoffOptions({ jitter: 1.5 })).toThrow(RangeError)
    expect(() => resolveBackoffOptions({ initialDelayMs: Number.POSITIVE_INFINITY })).toThrow(
      RangeError,
    )
  })
})
