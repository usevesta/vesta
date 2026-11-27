/**
 * Exponential backoff with jitter for reconnecting to the home.
 *
 * base(n)  = min(maxDelayMs, initialDelayMs * multiplier^n)
 * delay(n) = base(n) * (1 - jitter * r),  r in [0, 1)
 *
 * So every delay lies in `(base * (1 - jitter), base]` and never exceeds
 * `maxDelayMs`. Jitter spreads reconnects of many clients after the home
 * restarts, so they do not all knock at the same instant.
 */

export interface BackoffOptions {
  /** Delay before the first retry (attempt 0), in ms. */
  initialDelayMs: number
  /** Upper bound for any delay, in ms. */
  maxDelayMs: number
  /** Growth factor per attempt (>= 1). */
  multiplier: number
  /** Fraction of the delay that is randomized, between 0 and 1. */
  jitter: number
}

export const DEFAULT_BACKOFF: Readonly<BackoffOptions> = Object.freeze({
  initialDelayMs: 500,
  maxDelayMs: 30_000,
  multiplier: 2,
  jitter: 0.3,
})

export function resolveBackoffOptions(options: Partial<BackoffOptions> = {}): BackoffOptions {
  const resolved: BackoffOptions = { ...DEFAULT_BACKOFF, ...options }
  const { initialDelayMs, maxDelayMs, multiplier, jitter } = resolved
  if (!Number.isFinite(initialDelayMs) || initialDelayMs < 0) {
    throw new RangeError('initialDelayMs must be a finite number >= 0')
  }
  if (!Number.isFinite(maxDelayMs) || maxDelayMs < initialDelayMs) {
    throw new RangeError('maxDelayMs must be a finite number >= initialDelayMs')
  }
  if (!Number.isFinite(multiplier) || multiplier < 1) {
    throw new RangeError('multiplier must be a finite number >= 1')
  }
  if (!Number.isFinite(jitter) || jitter < 0 || jitter > 1) {
    throw new RangeError('jitter must be between 0 and 1')
  }
  return resolved
}

function normalizeAttempt(attempt: number): number {
  return Number.isFinite(attempt) && attempt > 0 ? Math.floor(attempt) : 0
}

/** Delay for `attempt` (0-based) before jitter is applied. */
export function backoffBaseDelay(attempt: number, options: Partial<BackoffOptions> = {}): number {
  const { initialDelayMs, maxDelayMs, multiplier } = resolveBackoffOptions(options)
  const raw = initialDelayMs * multiplier ** normalizeAttempt(attempt)
  // `raw` may overflow to Infinity for huge attempts; min() keeps the cap.
  return Math.min(maxDelayMs, raw)
}

/**
 * Delay for `attempt` (0-based) with jitter applied, in whole ms.
 * `random` must return a number in [0, 1) (defaults to Math.random).
 */
export function backoffDelay(
  attempt: number,
  options: Partial<BackoffOptions> = {},
  random: () => number = Math.random,
): number {
  const resolved = resolveBackoffOptions(options)
  const base = backoffBaseDelay(attempt, resolved)
  const r = Math.min(Math.max(random(), 0), 1)
  return Math.round(base * (1 - resolved.jitter * r))
}
