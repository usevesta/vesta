/**
 * One control's command in flight. A tap sends at most one command: until
 * it is done (the home answered, or it timed out), the control is busy and
 * refuses to send another, so a double tap never reaches the device twice.
 *
 * No React here (see `use-command.ts` for the hook), so it is unit tested
 * against the real live client.
 */

/**
 * A control's callback, e.g. `onPause` or `onModeChange(mode)`. A promise
 * it returns (the live client's command outcome) keeps the control busy
 * until it settles; anything else is fire-and-forget.
 */
export type CommandHandler<A extends readonly unknown[] = []> = (...args: A) => unknown

/**
 * Longest a control stays busy, whatever happens. The live client always
 * answers within its own timeout (5 s); this only guards against a
 * callback whose promise never settles.
 */
export const MAX_BUSY_MS = 15_000

export function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return (
    typeof value === 'object' && value !== null && 'then' in value && typeof value.then === 'function'
  )
}

export class CommandGate<A extends readonly unknown[] = []> {
  readonly #maxBusyMs: number
  #inFlight: A | null = null
  /** Bumped per command, so a late answer never frees a newer one. */
  #sequence = 0
  #timer: ReturnType<typeof setTimeout> | null = null
  readonly #listeners = new Set<() => void>()

  constructor(maxBusyMs: number = MAX_BUSY_MS) {
    this.#maxBusyMs = maxBusyMs
  }

  /** A command from this control is in flight. */
  get busy(): boolean {
    return this.#inFlight !== null
  }

  /** The arguments of the command in flight (e.g. which mode was picked), or null. */
  readonly getSnapshot = (): A | null => this.#inFlight

  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener)
    return () => {
      this.#listeners.delete(listener)
    }
  }

  /**
   * Calls `send` unless a command from this control is in flight. Returns
   * whether it was called. A promise it returns keeps the control busy
   * until it settles (resolved or rejected).
   */
  run(args: A, send: () => unknown): boolean {
    if (this.#inFlight !== null) return false
    const result = send()
    if (!isPromiseLike(result)) return true
    const sequence = ++this.#sequence
    this.#set(args)
    const done = () => {
      if (sequence === this.#sequence) this.#set(null)
    }
    this.#timer = setTimeout(done, this.#maxBusyMs)
    result.then(done, done)
    return true
  }

  #set(inFlight: A | null): void {
    if (this.#timer !== null) {
      clearTimeout(this.#timer)
      this.#timer = null
    }
    if (inFlight === null) this.#sequence += 1
    this.#inFlight = inFlight
    for (const listener of [...this.#listeners]) listener()
  }
}
