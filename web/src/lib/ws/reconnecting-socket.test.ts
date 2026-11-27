import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useConnectionStore, bindConnectionStore } from '@/lib/ws/connection-store'
import {
  ReconnectingSocket,
  type SocketFactory,
  type SocketHandlers,
  type WakeSignals,
} from '@/lib/ws/reconnecting-socket'
import { sameOriginSocketUrl } from '@/lib/ws/url'

interface FakeSocket {
  url: string
  handlers: SocketHandlers
  sent: string[]
  closed: boolean
}

function createHarness() {
  const sockets: FakeSocket[] = []
  const factory: SocketFactory = (url, handlers) => {
    const fake: FakeSocket = { url, handlers, sent: [], closed: false }
    sockets.push(fake)
    return {
      send: (data) => fake.sent.push(data),
      close: () => {
        fake.closed = true
      },
    }
  }
  let wake: (() => void) | null = null
  const wakeSignals: WakeSignals = {
    subscribe: (callback) => {
      wake = callback
      return () => {
        wake = null
      }
    },
  }
  const socket = new ReconnectingSocket({
    url: 'ws://127.0.0.1:5173/api/v1/ws',
    createSocket: factory,
    wakeSignals,
    // No jitter: delays equal the base schedule.
    random: () => 0,
    connectTimeoutMs: 5_000,
  })
  const last = (): FakeSocket => {
    const fake = sockets.at(-1)
    if (fake === undefined) throw new Error('no socket created yet')
    return fake
  }
  return {
    socket,
    sockets,
    last,
    wake: () => {
      wake?.()
    },
    isWakeSubscribed: () => wake !== null,
  }
}

describe('ReconnectingSocket', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('connects on start and reports open', () => {
    const h = createHarness()
    const opened = vi.fn()
    h.socket.onOpen(opened)
    h.socket.start()
    expect(h.socket.snapshot.status).toBe('connecting')
    h.last().handlers.onOpen()
    expect(h.socket.snapshot.status).toBe('open')
    expect(h.socket.snapshot.attempt).toBe(0)
    expect(opened).toHaveBeenCalledTimes(1)
  })

  it('retries with exponential backoff and resets after success', () => {
    const h = createHarness()
    h.socket.start()
    const delays = [500, 1_000, 2_000, 4_000]
    for (const [index, delay] of delays.entries()) {
      h.last().handlers.onClose()
      expect(h.socket.snapshot.status).toBe('waiting')
      expect(h.socket.snapshot.attempt).toBe(index + 1)
      vi.advanceTimersByTime(delay - 1)
      expect(h.sockets).toHaveLength(index + 1)
      vi.advanceTimersByTime(1)
      expect(h.sockets).toHaveLength(index + 2)
    }
    h.last().handlers.onOpen()
    expect(h.socket.snapshot.attempt).toBe(0)
    h.last().handlers.onClose()
    vi.advanceTimersByTime(500)
    expect(h.sockets).toHaveLength(delays.length + 2)
  })

  it('retries immediately on a wake signal (online / visible)', () => {
    const h = createHarness()
    h.socket.start()
    h.last().handlers.onClose()
    expect(h.sockets).toHaveLength(1)
    h.wake()
    expect(h.sockets).toHaveLength(2)
    expect(h.socket.snapshot.status).toBe('connecting')
  })

  it('ignores wake signals while connected', () => {
    const h = createHarness()
    h.socket.start()
    h.last().handlers.onOpen()
    h.wake()
    expect(h.sockets).toHaveLength(1)
  })

  it('abandons an attempt that never opens', () => {
    const h = createHarness()
    h.socket.start()
    vi.advanceTimersByTime(5_000)
    expect(h.sockets[0]?.closed).toBe(true)
    expect(h.socket.snapshot.status).toBe('waiting')
    vi.advanceTimersByTime(500)
    expect(h.sockets).toHaveLength(2)
  })

  it('delivers parsed JSON and drops invalid frames', () => {
    const h = createHarness()
    const received: unknown[] = []
    h.socket.subscribe((message) => received.push(message))
    h.socket.start()
    h.last().handlers.onOpen()
    h.last().handlers.onMessage('{"type":"snapshot","devices":[]}')
    h.last().handlers.onMessage('not json')
    h.last().handlers.onMessage(new ArrayBuffer(4))
    expect(received).toEqual([{ type: 'snapshot', devices: [] }])
  })

  it('keeps notifying listeners when one throws', () => {
    // The error is rethrown asynchronously; capture it instead for the test.
    const rethrow = vi.spyOn(globalThis, 'queueMicrotask').mockImplementation(() => undefined)
    const h = createHarness()
    const good = vi.fn()
    h.socket.subscribe(() => {
      throw new Error('listener bug')
    })
    h.socket.subscribe(good)
    h.socket.start()
    h.last().handlers.onOpen()
    h.last().handlers.onMessage('1')
    expect(good).toHaveBeenCalledWith(1)
    expect(rethrow).toHaveBeenCalledTimes(1)
    rethrow.mockRestore()
  })

  it('sends JSON only while open', () => {
    const h = createHarness()
    expect(h.socket.send({ type: 'ping' })).toBe(false)
    h.socket.start()
    expect(h.socket.send({ type: 'ping' })).toBe(false)
    h.last().handlers.onOpen()
    expect(h.socket.send({ type: 'ping' })).toBe(true)
    expect(h.last().sent).toEqual(['{"type":"ping"}'])
  })

  it('stops retrying after stop() and ignores stale callbacks', () => {
    const h = createHarness()
    h.socket.start()
    const first = h.last()
    first.handlers.onOpen()
    h.socket.stop()
    expect(first.closed).toBe(true)
    expect(h.isWakeSubscribed()).toBe(false)
    first.handlers.onClose()
    first.handlers.onMessage('{"late":true}')
    vi.advanceTimersByTime(60_000)
    expect(h.sockets).toHaveLength(1)
    expect(h.socket.snapshot.status).toBe('stopped')
  })

  it('reconnectNow skips the wait', () => {
    const h = createHarness()
    h.socket.start()
    h.last().handlers.onClose()
    h.socket.reconnectNow()
    expect(h.sockets).toHaveLength(2)
  })

  it('retries when the socket cannot be created', () => {
    let calls = 0
    const socket = new ReconnectingSocket({
      url: () => 'ws://invalid',
      createSocket: () => {
        calls += 1
        throw new SyntaxError('bad url')
      },
      wakeSignals: { subscribe: () => () => undefined },
      random: () => 0,
    })
    socket.start()
    expect(socket.snapshot.status).toBe('waiting')
    vi.advanceTimersByTime(500)
    expect(calls).toBe(2)
  })

  it('mirrors state into the connection store', () => {
    const h = createHarness()
    const unbind = bindConnectionStore(h.socket)
    h.socket.start()
    expect(useConnectionStore.getState().status).toBe('connecting')
    h.last().handlers.onOpen()
    expect(useConnectionStore.getState().status).toBe('open')
    unbind()
    h.last().handlers.onClose()
    expect(useConnectionStore.getState().status).toBe('open')
  })
})

describe('sameOriginSocketUrl', () => {
  it('maps http to ws and https to wss', () => {
    expect(sameOriginSocketUrl({ protocol: 'http:', host: '127.0.0.1:5173' })).toBe(
      'ws://127.0.0.1:5173/api/v1/ws',
    )
    expect(sameOriginSocketUrl({ protocol: 'https:', host: 'localhost' })).toBe(
      'wss://localhost/api/v1/ws',
    )
  })
})
