/**
 * Fires `onChange(true)` after `timeoutMs` without activity, and
 * `onChange(false)` on the first activity after that. A timeout of 0 (or
 * less) never goes idle.
 */
export class IdleTimer {
  readonly #timeoutMs: number
  readonly #onChange: (idle: boolean) => void
  #timer: ReturnType<typeof setTimeout> | null = null
  #idle = false
  #running = false

  constructor(timeoutMs: number, onChange: (idle: boolean) => void) {
    this.#timeoutMs = timeoutMs
    this.#onChange = onChange
  }

  get idle(): boolean {
    return this.#idle
  }

  start(): void {
    this.#running = true
    this.#schedule()
  }

  /** Someone touched the screen (or something worth waking up for happened). */
  activity(): void {
    if (!this.#running) return
    if (this.#idle) {
      this.#idle = false
      this.#onChange(false)
    }
    this.#schedule()
  }

  stop(): void {
    this.#running = false
    this.#clear()
    this.#idle = false
  }

  #schedule(): void {
    this.#clear()
    if (!Number.isFinite(this.#timeoutMs) || this.#timeoutMs <= 0) return
    this.#timer = setTimeout(() => {
      this.#timer = null
      this.#idle = true
      this.#onChange(true)
    }, this.#timeoutMs)
  }

  #clear(): void {
    if (this.#timer !== null) clearTimeout(this.#timer)
    this.#timer = null
  }
}
