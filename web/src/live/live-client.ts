/**
 * The connection to the home: one reconnecting WebSocket, decoded with the
 * generated protocol types and applied to a device store. Commands are
 * optimistic: the prediction shows as pending at once, resolves when the
 * device confirms, and reverts (with an error) when it does not.
 *
 * No React or DOM here, so the same client runs in the end-to-end test.
 */
import type {
  ClientMessage,
  Command,
  CommandError,
  EndpointEvent,
  RejectReason,
  ServerMessage,
  Target,
} from '@/api/generated'
import { decodeServerMessage } from '@/api/protocol'
import {
  acceptPending,
  addPending,
  applyAvailability,
  applySnapshot,
  applyStateChange,
  findPending,
  markUnsynced,
  removePending,
  selectEndpointView,
  type DeviceData,
  type DeviceStore,
} from '@/live/device-store'
import { predict } from '@/live/optimistic'
import {
  ReconnectingSocket,
  type ReconnectingSocketOptions,
} from '@/lib/ws/reconnecting-socket'

/** How long a command may stay unconfirmed before the screen reverts it. */
export const COMMAND_TIMEOUT_MS = 5_000
/**
 * The same for slow commands (saving or reading a charger's hours may go
 * through the internet). Matches `SLOW_COMMAND_TIMEOUT` on the home
 * (`Command::is_slow`).
 */
export const SLOW_COMMAND_TIMEOUT_MS = 20_000

/** Commands the home gives longer (`Command::is_slow`). */
export function isSlowCommand(command: Command): boolean {
  return (
    command.capability === 'ev_charger' &&
    (command.action === 'set_charging_schedule' || command.action === 'refresh_charging_schedule')
  )
}

/**
 * Why a command did not happen: the home's error codes, plus two the
 * screen detects itself (`not_connected`: nothing was sent; `no_response`:
 * no answer within the timeout).
 */
export type CommandFailureReason = CommandError['code'] | 'not_connected' | 'no_response'

export interface CommandFailure {
  readonly target: Target
  readonly command: Command
  readonly reason: CommandFailureReason
  /** Only for `rejected`. */
  readonly rejectReason?: RejectReason
  /** Name to show in the message (the widget's label). */
  readonly label?: string
}

export type CommandOutcome = { readonly ok: true } | ({ readonly ok: false } & CommandFailure)

export interface SendCommandOptions {
  /** True only after the person confirmed a sensitive command. */
  confirmed?: boolean
  /** Name of the thing being controlled, for error messages. */
  label?: string
  /**
   * The control says itself when this fails (e.g. reading a charger's
   * hours): the outcome still says why, but `onCommandFailed` is not told.
   */
  quiet?: boolean
}

export interface LiveClientOptions extends Pick<
  ReconnectingSocketOptions,
  'url' | 'createSocket' | 'wakeSignals' | 'backoff' | 'connectTimeoutMs'
> {
  store: DeviceStore
  commandTimeoutMs?: number
  slowCommandTimeoutMs?: number
  /** Show a failure to the person (a translated toast in the app). */
  onCommandFailed?: (failure: CommandFailure) => void
  newRequestId?: () => string
}

export type EventListener = (event: EndpointEvent) => void
export type ServerMessageListener = (message: ServerMessage) => void

interface InFlight {
  readonly target: Target
  readonly command: Command
  readonly confirmed: boolean
  readonly label: string | undefined
  readonly quiet: boolean
  readonly outcome: Promise<CommandOutcome>
  readonly resolve: (outcome: CommandOutcome) => void
  timer: ReturnType<typeof setTimeout> | null
  /** The home's `command_result` arrived (the promise is settled). */
  answered: boolean
}

function sameValue(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false
  if (Array.isArray(a) !== Array.isArray(b)) return false
  const keysA = Object.keys(a)
  const keysB = Object.keys(b)
  return (
    keysA.length === keysB.length &&
    keysA.every(
      (key) => Object.hasOwn(b, key) && sameValue((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]),
    )
  )
}

function randomToken(): string {
  const bytes = new Uint8Array(4)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

/** `c-<session>-<n>`: unique per screen session, well under the 64-char limit. */
export function requestIdGenerator(session: string = randomToken()): () => string {
  let counter = 0
  return () => {
    counter += 1
    return `c-${session}-${String(counter)}`
  }
}

export class LiveClient {
  readonly socket: ReconnectingSocket
  readonly #store: DeviceStore
  readonly #timeoutMs: number
  readonly #slowTimeoutMs: number
  readonly #onCommandFailed: (failure: CommandFailure) => void
  readonly #newRequestId: () => string
  readonly #inFlight = new Map<string, InFlight>()
  readonly #eventListeners = new Set<EventListener>()
  readonly #messageListeners = new Set<ServerMessageListener>()
  #unsubscribers: (() => void)[] = []

  constructor(options: LiveClientOptions) {
    this.socket = new ReconnectingSocket(options)
    this.#store = options.store
    this.#timeoutMs = options.commandTimeoutMs ?? COMMAND_TIMEOUT_MS
    this.#slowTimeoutMs = options.slowCommandTimeoutMs ?? SLOW_COMMAND_TIMEOUT_MS
    this.#onCommandFailed = options.onCommandFailed ?? (() => undefined)
    this.#newRequestId = options.newRequestId ?? requestIdGenerator()
  }

  get store(): DeviceStore {
    return this.#store
  }

  /** Connect (and keep reconnecting). Safe to call again after `stop()`. */
  start(): void {
    if (this.#unsubscribers.length === 0) {
      this.#unsubscribers = [
        this.socket.subscribe((raw) => {
          const message = decodeServerMessage(raw)
          if (message !== null) this.#handle(message)
        }),
        // The home sends a fresh snapshot first on every connection; until
        // it arrives, the data on screen may be stale.
        this.socket.onOpen(() => {
          this.#update(markUnsynced)
        }),
        this.socket.onStateChange((snapshot) => {
          if (snapshot.status !== 'open') this.#update(markUnsynced)
        }),
      ]
    }
    this.socket.start()
  }

  stop(): void {
    this.socket.stop()
    for (const unsubscribe of this.#unsubscribers) unsubscribe()
    this.#unsubscribers = []
  }

  /** Doorbell presses, motion, ... Returns an unsubscribe function. */
  onEvent(listener: EventListener): () => void {
    this.#eventListeners.add(listener)
    return () => {
      this.#eventListeners.delete(listener)
    }
  }

  /** Every decoded message, after the store was updated (tests, diagnostics). */
  onMessage(listener: ServerMessageListener): () => void {
    this.#messageListeners.add(listener)
    return () => {
      this.#messageListeners.delete(listener)
    }
  }

  /**
   * Send `command` to `target` optimistically. Resolves with `ok: true` once
   * the home accepted it (or the device visibly did it), or with the reason
   * it failed. Failures are also reported to `onCommandFailed`.
   */
  sendCommand(target: Target, command: Command, options: SendCommandOptions = {}): Promise<CommandOutcome> {
    const { confirmed = false, label, quiet = false } = options
    const data = this.#store.getState()
    if (!data.synced || this.socket.snapshot.status !== 'open') {
      return Promise.resolve(this.#fail({ target, command, label, reason: 'not_connected' }, quiet))
    }
    // The same command is already on its way (e.g. the same button on the
    // tile and in its dialog): never send it twice, share its outcome.
    const twin = this.#twin(target, command, confirmed)
    if (twin !== undefined) return twin.outcome

    const requestId = this.#newRequestId()
    const { predicted, confirmsWith } = predict(command, selectEndpointView(data, target)?.endpoint)
    this.#update((current) =>
      addPending(current, {
        requestId,
        target,
        predicted,
        confirmsWith,
        accepted: false,
        observed: false,
        confirmed: false,
      }),
    )

    const message: ClientMessage = { type: 'command', request_id: requestId, target, command, confirmed }
    if (!this.socket.send(message)) {
      this.#update((current) => removePending(current, requestId))
      return Promise.resolve(this.#fail({ target, command, label, reason: 'not_connected' }, quiet))
    }

    let resolve: (outcome: CommandOutcome) => void = () => undefined
    const outcome = new Promise<CommandOutcome>((settle) => {
      resolve = settle
    })
    const entry: InFlight = { target, command, confirmed, label, quiet, outcome, resolve, timer: null, answered: false }
    entry.timer = setTimeout(
      () => {
        this.#onTimeout(requestId)
      },
      isSlowCommand(command) ? this.#slowTimeoutMs : this.#timeoutMs,
    )
    this.#inFlight.set(requestId, entry)
    return outcome
  }

  /** An identical command still waiting for the home's answer. */
  #twin(target: Target, command: Command, confirmed: boolean): InFlight | undefined {
    for (const entry of this.#inFlight.values()) {
      if (!entry.answered && entry.target === target && entry.confirmed === confirmed && sameValue(entry.command, command)) {
        return entry
      }
    }
    return undefined
  }

  #update(reducer: (data: DeviceData) => DeviceData): void {
    const current = this.#store.getState()
    const next = reducer(current)
    if (next !== current) this.#store.setState(next, true)
  }

  #fail(failure: CommandFailure, quiet = false): CommandOutcome {
    if (!quiet) this.#onCommandFailed(failure)
    return { ok: false, ...failure }
  }

  #handle(message: ServerMessage): void {
    switch (message.type) {
      case 'snapshot':
        this.#update((data) => applySnapshot(data, message))
        break
      case 'state':
        this.#update((data) => applyStateChange(data, message))
        break
      case 'availability':
        this.#update((data) => applyAvailability(data, message))
        break
      case 'event':
        for (const listener of [...this.#eventListeners]) listener(message)
        break
      case 'command_result':
        this.#onResult(message.request_id, message.ok ? null : (message.error ?? { code: 'device' }))
        break
    }
    for (const listener of [...this.#messageListeners]) listener(message)
    this.#forgetSettled()
  }

  #onResult(requestId: string, error: CommandError | null): void {
    const entry = this.#inFlight.get(requestId)
    if (entry === undefined || entry.answered) return
    entry.answered = true
    if (error === null) {
      this.#update((data) => acceptPending(data, requestId))
      entry.resolve({ ok: true })
      return
    }
    this.#finish(requestId)
    this.#update((data) => removePending(data, requestId))
    entry.resolve(
      this.#fail(
        {
          target: entry.target,
          command: entry.command,
          label: entry.label,
          reason: error.code,
          ...(error.code === 'rejected' ? { rejectReason: error.reason } : {}),
        },
        entry.quiet,
      ),
    )
  }

  #onTimeout(requestId: string): void {
    const entry = this.#inFlight.get(requestId)
    if (entry === undefined) return
    this.#finish(requestId)
    const stillPending = findPending(this.#store.getState(), requestId) !== undefined
    this.#update((data) => removePending(data, requestId))
    if (entry.answered) return
    // No answer (e.g. the connection dropped): if the device visibly did
    // it, that is good enough; otherwise revert and say so.
    entry.resolve(
      stillPending
        ? this.#fail(
            { target: entry.target, command: entry.command, label: entry.label, reason: 'no_response' },
            entry.quiet,
          )
        : { ok: true },
    )
  }

  /** Commands answered `ok` whose prediction has been confirmed need no timer. */
  #forgetSettled(): void {
    if (this.#inFlight.size === 0) return
    const data = this.#store.getState()
    for (const [requestId, entry] of this.#inFlight) {
      if (entry.answered && findPending(data, requestId) === undefined) this.#finish(requestId)
    }
  }

  #finish(requestId: string): void {
    const entry = this.#inFlight.get(requestId)
    if (entry?.timer != null) clearTimeout(entry.timer)
    this.#inFlight.delete(requestId)
  }
}

export function createLiveClient(options: LiveClientOptions): LiveClient {
  return new LiveClient(options)
}
