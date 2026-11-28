import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { EndpointEvent } from '@/api/generated'
import { commandErrorKey } from '@/live/command-errors'
import { createDeviceStore, selectEndpointView } from '@/live/device-store'
import {
  COMMAND_TIMEOUT_MS,
  createLiveClient,
  requestIdGenerator,
  SLOW_COMMAND_TIMEOUT_MS,
  type CommandFailure,
} from '@/live/live-client'
import { FakeSockets, NO_WAKE_SIGNALS, sampleSnapshot } from '@/live/test-fixtures'

const FAN_LIGHT = 'living_room_fan/light'

function setup() {
  const sockets = new FakeSockets()
  const store = createDeviceStore()
  const failures: CommandFailure[] = []
  const client = createLiveClient({
    url: 'ws://localhost/api/v1/ws',
    createSocket: sockets.factory,
    wakeSignals: NO_WAKE_SIGNALS,
    store,
    newRequestId: requestIdGenerator('test'),
    onCommandFailed: (failure) => failures.push(failure),
  })
  client.start()
  sockets.open()
  sockets.receive({ type: 'snapshot', ...sampleSnapshot() })
  const view = () => selectEndpointView(store.getState(), FAN_LIGHT)
  return { sockets, store, failures, client, view }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('live client', () => {
  it('waits for a fresh snapshot after every (re)connect', () => {
    const { sockets, store } = setup()
    expect(store.getState().synced).toBe(true)
    sockets.drop()
    expect(store.getState().synced).toBe(false)
    vi.advanceTimersByTime(1_000)
    sockets.open()
    expect(store.getState().synced).toBe(false)
    sockets.receive({ type: 'snapshot', ...sampleSnapshot() })
    expect(store.getState().synced).toBe(true)
  })

  it('sends typed commands with unique request ids', () => {
    const { sockets, client } = setup()
    void client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'on' })
    void client.sendCommand('front_door_strike/main', { capability: 'trigger', action: 'fire' }, { confirmed: true })
    expect(sockets.sent).toEqual([
      {
        type: 'command',
        request_id: 'c-test-1',
        target: FAN_LIGHT,
        command: { capability: 'on_off', action: 'on' },
        confirmed: false,
      },
      {
        type: 'command',
        request_id: 'c-test-2',
        target: 'front_door_strike/main',
        command: { capability: 'trigger', action: 'fire' },
        confirmed: true,
      },
    ])
  })

  it('shows the change at once and resolves when the device confirms', async () => {
    const { sockets, client, view, failures } = setup()
    const outcome = client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'on' })
    expect(view()).toMatchObject({ pending: true, endpoint: { capabilities: { on_off: { state: { on: true } } } } })

    // Results may come before or after the state they cause.
    sockets.receive({ type: 'command_result', request_id: 'c-test-1', ok: true })
    await expect(outcome).resolves.toEqual({ ok: true })
    expect(view()?.pending).toBe(true)
    sockets.receive({ type: 'state', target: FAN_LIGHT, capability: 'on_off', state: { on: true } })
    expect(view()).toMatchObject({ pending: false, endpoint: { capabilities: { on_off: { state: { on: true } } } } })

    vi.advanceTimersByTime(COMMAND_TIMEOUT_MS * 2)
    expect(failures).toEqual([])
  })

  it('reverts and reports the reason when the home says no', async () => {
    const { sockets, client, view, failures } = setup()
    const outcome = client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'on' }, { label: 'Fan Light' })
    sockets.receive({ type: 'command_result', request_id: 'c-test-1', ok: false, error: { code: 'offline' } })
    await expect(outcome).resolves.toMatchObject({ ok: false, reason: 'offline', label: 'Fan Light' })
    expect(view()).toMatchObject({ pending: false, endpoint: { capabilities: { on_off: { state: { on: false } } } } })
    expect(failures.map(commandErrorKey)).toEqual(['commandError.offline'])
  })

  it('passes the reason for rejected commands', async () => {
    const { sockets, client } = setup()
    const outcome = client.sendCommand('garage_door/main', { capability: 'cover', action: 'open' })
    sockets.receive({
      type: 'command_result',
      request_id: 'c-test-1',
      ok: false,
      error: { code: 'rejected', reason: 'confirmation_required' },
    })
    await expect(outcome).resolves.toMatchObject({ ok: false, reason: 'rejected', rejectReason: 'confirmation_required' })
  })

  it('reverts after 5 s without an answer', async () => {
    const { client, view, failures } = setup()
    const outcome = client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'on' })
    vi.advanceTimersByTime(COMMAND_TIMEOUT_MS - 1)
    expect(view()?.pending).toBe(true)
    vi.advanceTimersByTime(1)
    await expect(outcome).resolves.toMatchObject({ ok: false, reason: 'no_response' })
    expect(view()).toMatchObject({ pending: false, endpoint: { capabilities: { on_off: { state: { on: false } } } } })
    expect(failures.map(commandErrorKey)).toEqual(['widget.commandFailed'])
  })

  it('gives saving charging hours longer, and says what to check when it fails', async () => {
    const { client, failures } = setup()
    const save = client.sendCommand('car_charger/charger', {
      capability: 'ev_charger',
      action: 'set_charging_schedule',
      enabled: true,
      windows: [{ start: '22:00', end: '06:00', days: ['saturday'] }],
    })
    vi.advanceTimersByTime(COMMAND_TIMEOUT_MS)
    expect(failures).toEqual([])
    vi.advanceTimersByTime(SLOW_COMMAND_TIMEOUT_MS - COMMAND_TIMEOUT_MS)
    await expect(save).resolves.toMatchObject({ ok: false, reason: 'no_response' })
    expect(failures.map(commandErrorKey)).toEqual(['ev.hours.saveFailed'])
    // Other reasons keep their own message.
    expect(commandErrorKey({ reason: 'offline', command: { capability: 'ev_charger', action: 'pause' } })).toBe(
      'commandError.offline',
    )
  })

  it('counts a visible change as success even if the answer got lost', async () => {
    const { sockets, client, failures } = setup()
    const outcome = client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'on' })
    sockets.drop()
    vi.advanceTimersByTime(600)
    sockets.open()
    const snapshot = sampleSnapshot()
    const light = snapshot.devices[1]?.endpoints[1]
    if (light?.capabilities.on_off === undefined) throw new Error('fixture changed')
    light.capabilities.on_off.state = { on: true }
    sockets.receive({ type: 'snapshot', ...snapshot })
    vi.advanceTimersByTime(COMMAND_TIMEOUT_MS)
    await expect(outcome).resolves.toEqual({ ok: true })
    expect(failures).toEqual([])
  })

  it('refuses to send while disconnected', async () => {
    const { sockets, client, failures, view } = setup()
    sockets.drop()
    await expect(client.sendCommand(FAN_LIGHT, { capability: 'on_off', action: 'on' })).resolves.toMatchObject({
      ok: false,
      reason: 'not_connected',
    })
    expect(view()?.pending).toBe(false)
    expect(failures.map(commandErrorKey)).toEqual(['commandError.notConnected'])
  })

  it('delivers events and ignores garbage', () => {
    const { sockets, client, store } = setup()
    const events: EndpointEvent[] = []
    client.onEvent((event) => events.push(event))
    const before = store.getState()
    sockets.receive({ type: 'mystery' })
    sockets.receive({ type: 'state', target: FAN_LIGHT, capability: 'on_off' })
    expect(store.getState()).toBe(before)
    sockets.receive({ type: 'event', target: 'front_doorbell/main', event: 'doorbell.pressed', at: '2026-01-01T12:00:00Z' })
    expect(events).toEqual([
      { type: 'event', target: 'front_doorbell/main', event: 'doorbell.pressed', at: '2026-01-01T12:00:00Z' },
    ])
  })
})
