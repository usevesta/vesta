import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createDeviceStore } from '@/live/device-store'
import { COMMAND_TIMEOUT_MS, createLiveClient, requestIdGenerator } from '@/live/live-client'
import { FakeSockets, NO_WAKE_SIGNALS, sampleSnapshot } from '@/live/test-fixtures'
import { CommandGate, MAX_BUSY_MS } from '@/lib/command-gate'

const FAN_LIGHT = 'living_room_fan/light'

function deferred() {
  let resolve: (value: unknown) => void = () => undefined
  let reject: (reason: unknown) => void = () => undefined
  const promise = new Promise((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

/** Lets pending promise callbacks run. */
async function flush() {
  for (let i = 0; i < 5; i += 1) await Promise.resolve()
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('command gate', () => {
  it('refuses a second tap while the first command is on its way', async () => {
    const gate = new CommandGate<[string]>()
    const changes: (readonly [string] | null)[] = []
    gate.subscribe(() => changes.push(gate.getSnapshot()))
    const sent: string[] = []
    const first = deferred()

    expect(gate.run(['solar'], () => (sent.push('solar'), first.promise))).toBe(true)
    expect(gate.busy).toBe(true)
    expect(gate.getSnapshot()).toEqual(['solar'])
    expect(gate.run(['manual'], () => sent.push('manual'))).toBe(false)
    expect(gate.run(['solar'], () => sent.push('solar'))).toBe(false)
    expect(sent).toEqual(['solar'])

    first.resolve({ ok: true })
    await flush()
    expect(gate.busy).toBe(false)
    expect(changes).toEqual([['solar'], null])
    expect(gate.run(['manual'], () => sent.push('manual'))).toBe(true)
    expect(sent).toEqual(['solar', 'manual'])
  })

  it('is free again when the command failed', async () => {
    const gate = new CommandGate()
    const command = deferred()
    gate.run([], () => command.promise)
    command.reject(new Error('no answer'))
    await flush()
    expect(gate.busy).toBe(false)
  })

  it('is never busy for a callback with nothing to wait for', () => {
    const gate = new CommandGate()
    expect(gate.run([], () => undefined)).toBe(true)
    expect(gate.busy).toBe(false)
  })

  it('frees itself if a callback never settles', async () => {
    const gate = new CommandGate()
    const never = new Promise(() => undefined)
    gate.run([], () => never)
    vi.advanceTimersByTime(MAX_BUSY_MS - 1)
    expect(gate.busy).toBe(true)
    vi.advanceTimersByTime(1)
    expect(gate.busy).toBe(false)

    // A late answer to the old command never frees a newer one.
    const late = deferred()
    const gate2 = new CommandGate(100)
    gate2.run([], () => late.promise)
    vi.advanceTimersByTime(100)
    const newer = deferred()
    gate2.run([], () => newer.promise)
    late.resolve(undefined)
    await flush()
    expect(gate2.busy).toBe(true)
  })
})

describe('command gate with the live client', () => {
  function setup() {
    const sockets = new FakeSockets()
    const store = createDeviceStore()
    const client = createLiveClient({
      url: 'ws://localhost/api/v1/ws',
      createSocket: sockets.factory,
      wakeSignals: NO_WAKE_SIGNALS,
      store,
      newRequestId: requestIdGenerator('test'),
    })
    client.start()
    sockets.open()
    sockets.receive({ type: 'snapshot', ...sampleSnapshot() })
    const toggle = new CommandGate<[boolean]>()
    const tap = (on: boolean) =>
      toggle.run([on], () => client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: on ? 'on' : 'off' }))
    return { sockets, client, toggle, tap }
  }

  it('sends one command for a double tap and is free once the home answered', async () => {
    const { sockets, toggle, tap } = setup()
    expect(tap(true)).toBe(true)
    expect(tap(true)).toBe(false)
    expect(tap(false)).toBe(false)
    expect(sockets.sent).toHaveLength(1)
    expect(toggle.busy).toBe(true)

    sockets.receive({ type: 'state', target: FAN_LIGHT, capability: 'on_off', state: { on: true } })
    sockets.receive({ type: 'command_result', request_id: 'c-test-1', ok: true })
    await flush()
    expect(toggle.busy).toBe(false)
    expect(tap(false)).toBe(true)
    expect(sockets.sent).toHaveLength(2)
  })

  it('is free again when the home says no', async () => {
    const { sockets, toggle, tap } = setup()
    tap(true)
    sockets.receive({ type: 'command_result', request_id: 'c-test-1', ok: false, error: { code: 'offline' } })
    await flush()
    expect(toggle.busy).toBe(false)
  })

  it('is free again after the timeout, when the screen reverts', async () => {
    const { sockets, toggle, tap } = setup()
    tap(true)
    vi.advanceTimersByTime(COMMAND_TIMEOUT_MS - 1)
    await flush()
    expect(toggle.busy).toBe(true)
    vi.advanceTimersByTime(1)
    await flush()
    expect(toggle.busy).toBe(false)
    expect(sockets.sent).toHaveLength(1)
  })

  it('never sends the same command twice from two buttons (tile and dialog)', async () => {
    const { sockets, client } = setup()
    const tile = new CommandGate()
    const dialog = new CommandGate()
    const pause = () => client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'off' })
    tile.run([], pause)
    dialog.run([], pause)
    expect(sockets.sent).toHaveLength(1)
    expect(tile.busy && dialog.busy).toBe(true)
    sockets.receive({ type: 'command_result', request_id: 'c-test-1', ok: true })
    await flush()
    expect(tile.busy || dialog.busy).toBe(false)
  })
})
