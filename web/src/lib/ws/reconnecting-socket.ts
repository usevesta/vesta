/**
 * Generic reconnecting WebSocket client.
 *
 * - Exponential backoff with jitter between attempts (capped, see backoff.ts).
 * - Retries immediately when the device comes back online or the app becomes
 *   visible again (Wi-Fi <-> mobile data handoffs, waking a tablet).
 * - Gives up on a connection attempt that hangs (connect timeout).
 * - Delivers every text frame parsed as JSON, typed `unknown`: decoding into
 *   protocol types is the caller's job (types come from `src/api/generated`).
 *
 * Protocol-agnostic on purpose: it knows nothing about snapshots or commands.
 * Use `onOpen` to request a full snapshot after every (re)connect.
 */
import { backoffDelay, resolveBackoffOptions, type BackoffOptions } from '@/lib/ws/backoff'

export type ConnectionStatus =
  /** Created, `start()` not called yet. */
  | 'idle'
  /** A connection attempt is in progress. */
  | 'connecting'
  /** Connected; messages flow. */
  | 'open'
  /** Disconnected; the next attempt starts at `nextRetryAt`. */
  | 'waiting'
  /** `stop()` was called; no further attempts. */
  | 'stopped'

export interface ConnectionSnapshot {
  readonly status: ConnectionStatus
  /** Consecutive failed attempts since the last successful connection. */
  readonly attempt: number
  /** When the next attempt starts (epoch ms) while `waiting`, else null. */
  readonly nextRetryAt: number | null
  /** When the last connection opened (epoch ms), or null if never. */
  readonly lastOpenedAt: number | null
}

export const INITIAL_CONNECTION_SNAPSHOT: ConnectionSnapshot = Object.freeze({
  status: 'idle',
  attempt: 0,
  nextRetryAt: null,
  lastOpenedAt: null,
})

/** Callbacks a socket implementation reports to. */
export interface SocketHandlers {
  onOpen: () => void
  /** Raw frame payload (string for text frames). */
  onMessage: (data: unknown) => void
  /** Called once when the socket closes or fails. */
  onClose: () => void
}

export interface SocketHandle {
  send: (data: string) => void
  /** Close and detach: no handler may fire after this returns. */
  close: () => void
}

export type SocketFactory = (url: string, handlers: SocketHandlers) => SocketHandle

/** Sources of "now is a good moment to retry" hints. */
export interface WakeSignals {
  subscribe: (wake: () => void) => () => void
}

export interface ReconnectingSocketOptions {
  /** Absolute ws:// or wss:// URL, or a function evaluated per attempt. */
  url: string | (() => string)
  backoff?: Partial<BackoffOptions>
  /** Abandon an attempt that has not opened after this long. Default 10 s. */
  connectTimeoutMs?: number
  /** Defaults to the browser WebSocket. */
  createSocket?: SocketFactory
  /** Defaults to `online` and `visibilitychange` events. */
  wakeSignals?: WakeSignals
  /** Injectable for tests. */
  random?: () => number
  now?: () => number
}

export type MessageListener = (message: unknown) => void
export type StateListener = (snapshot: ConnectionSnapshot) => void

export const DEFAULT_CONNECT_TIMEOUT_MS = 10_000

export const browserSocketFactory: SocketFactory = (url, handlers) => {
  const socket = new WebSocket(url)
  socket.onopen = () => {
    handlers.onOpen()
  }
  socket.onmessage = (event: MessageEvent<unknown>) => {
    handlers.onMessage(event.data)
  }
  socket.onclose = () => {
    handlers.onClose()
  }
  // An `error` event is always followed by `close`; handle it there.
  socket.onerror = null
  return {
    send: (data) => {
      socket.send(data)
    },
    close: () => {
      socket.onopen = null
      socket.onmessage = null
      socket.onclose = null
      if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) {
        socket.close()
      }
    },
  }
}

export const browserWakeSignals: WakeSignals = {
  subscribe(wake) {
    if (typeof window === 'undefined' || typeof document === 'undefined') {
      return () => undefined
    }
    const onOnline = () => {
      wake()
    }
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') wake()
    }
    window.addEventListener('online', onOnline)
    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => {
      window.removeEventListener('online', onOnline)
      document.removeEventListener('visibilitychange', onVisibilityChange)
    }
  },
}

/** Run every listener even if one throws; rethrow asynchronously so it is still reported. */
function notifyAll<T>(listeners: Iterable<(value: T) => void>, value: T): void {
  for (const listener of [...listeners]) {
    try {
      listener(value)
    } catch (error: unknown) {
      queueMicrotask(() => {
        throw error
      })
    }
  }
}

export class ReconnectingSocket {
  readonly #url: string | (() => string)
  readonly #backoff: BackoffOptions
  readonly #connectTimeoutMs: number
  readonly #createSocket: SocketFactory
  readonly #wakeSignals: WakeSignals
  readonly #random: () => number
  readonly #now: () => number

  #snapshot: ConnectionSnapshot = INITIAL_CONNECTION_SNAPSHOT
  #socket: SocketHandle | null = null
  /** Incremented whenever the current socket is abandoned; stale callbacks compare against it. */
  #generation = 0
  #retryTimer: ReturnType<typeof setTimeout> | null = null
  #connectTimer: ReturnType<typeof setTimeout> | null = null
  #unsubscribeWake: (() => void) | null = null

  readonly #messageListeners = new Set<MessageListener>()
  readonly #stateListeners = new Set<StateListener>()
  readonly #openListeners = new Set<() => void>()

  constructor(options: ReconnectingSocketOptions) {
    this.#url = options.url
    this.#backoff = resolveBackoffOptions(options.backoff)
    this.#connectTimeoutMs = options.connectTimeoutMs ?? DEFAULT_CONNECT_TIMEOUT_MS
    this.#createSocket = options.createSocket ?? browserSocketFactory
    this.#wakeSignals = options.wakeSignals ?? browserWakeSignals
    this.#random = options.random ?? Math.random
    this.#now = options.now ?? Date.now
  }

  get snapshot(): ConnectionSnapshot {
    return this.#snapshot
  }

  /** Start connecting. Safe to call more than once. */
  start(): void {
    const { status } = this.#snapshot
    if (status === 'connecting' || status === 'open' || status === 'waiting') return
    this.#unsubscribeWake = this.#wakeSignals.subscribe(() => {
      this.#onWake()
    })
    this.#connect()
  }

  /** Close the connection and stop retrying. `start()` can resume later. */
  stop(): void {
    this.#clearRetryTimer()
    this.#clearConnectTimer()
    this.#unsubscribeWake?.()
    this.#unsubscribeWake = null
    this.#dropSocket()
    this.#setSnapshot({ status: 'stopped', nextRetryAt: null })
  }

  /** Skip the remaining wait and retry now (e.g. a "Try now" button). */
  reconnectNow(): void {
    if (this.#snapshot.status === 'waiting') this.#connect()
  }

  /** Send a JSON-serializable message. Returns false if not connected. */
  send(message: unknown): boolean {
    if (this.#snapshot.status !== 'open' || this.#socket === null) return false
    try {
      this.#socket.send(JSON.stringify(message))
      return true
    } catch {
      return false
    }
  }

  /** Receive every parsed JSON message. Returns an unsubscribe function. */
  subscribe(listener: MessageListener): () => void {
    this.#messageListeners.add(listener)
    return () => {
      this.#messageListeners.delete(listener)
    }
  }

  /** Observe connection state changes. Returns an unsubscribe function. */
  onStateChange(listener: StateListener): () => void {
    this.#stateListeners.add(listener)
    return () => {
      this.#stateListeners.delete(listener)
    }
  }

  /** Called after every successful (re)connect, e.g. to request a snapshot. */
  onOpen(listener: () => void): () => void {
    this.#openListeners.add(listener)
    return () => {
      this.#openListeners.delete(listener)
    }
  }

  #connect(): void {
    this.#clearRetryTimer()
    this.#clearConnectTimer()
    this.#dropSocket()

    const generation = this.#generation
    const isCurrent = () => generation === this.#generation
    this.#setSnapshot({ status: 'connecting', nextRetryAt: null })

    const handlers: SocketHandlers = {
      onOpen: () => {
        if (!isCurrent()) return
        this.#clearConnectTimer()
        this.#setSnapshot({ status: 'open', attempt: 0, lastOpenedAt: this.#now() })
        notifyAll(this.#openListeners, undefined)
      },
      onMessage: (data) => {
        if (!isCurrent()) return
        this.#deliver(data)
      },
      onClose: () => {
        if (!isCurrent()) return
        this.#generation += 1
        this.#socket = null
        this.#scheduleRetry()
      },
    }

    let socket: SocketHandle
    try {
      const url = typeof this.#url === 'function' ? this.#url() : this.#url
      socket = this.#createSocket(url, handlers)
    } catch {
      // Invalid URL or the platform refused to create a socket: retry later.
      if (isCurrent()) this.#scheduleRetry()
      return
    }

    if (!isCurrent()) {
      // The factory reported a close synchronously; that path already rescheduled.
      socket.close()
      return
    }
    this.#socket = socket
    if (this.#snapshot.status === 'connecting') {
      this.#connectTimer = setTimeout(() => {
        this.#connectTimer = null
        if (!isCurrent()) return
        this.#dropSocket()
        this.#scheduleRetry()
      }, this.#connectTimeoutMs)
    }
  }

  #scheduleRetry(): void {
    if (this.#snapshot.status === 'stopped') return
    this.#clearRetryTimer()
    this.#clearConnectTimer()
    const { attempt } = this.#snapshot
    const delay = backoffDelay(attempt, this.#backoff, this.#random)
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null
      this.#connect()
    }, delay)
    this.#setSnapshot({ status: 'waiting', attempt: attempt + 1, nextRetryAt: this.#now() + delay })
  }

  #onWake(): void {
    // Keep the attempt count: if this retry fails too, backoff carries on.
    if (this.#snapshot.status === 'waiting') this.#connect()
  }

  #deliver(data: unknown): void {
    // Binary frames are not part of the protocol.
    if (typeof data !== 'string') return
    let message: unknown
    try {
      message = JSON.parse(data)
    } catch {
      return
    }
    notifyAll(this.#messageListeners, message)
  }

  /** Abandon the current socket without triggering a retry. */
  #dropSocket(): void {
    this.#generation += 1
    const socket = this.#socket
    this.#socket = null
    socket?.close()
  }

  #clearRetryTimer(): void {
    if (this.#retryTimer !== null) {
      clearTimeout(this.#retryTimer)
      this.#retryTimer = null
    }
  }

  #clearConnectTimer(): void {
    if (this.#connectTimer !== null) {
      clearTimeout(this.#connectTimer)
      this.#connectTimer = null
    }
  }

  #setSnapshot(patch: Partial<ConnectionSnapshot>): void {
    this.#snapshot = Object.freeze({ ...this.#snapshot, ...patch })
    notifyAll(this.#stateListeners, this.#snapshot)
  }
}
