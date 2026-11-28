/**
 * End to end: the simulated plugs (one switchable, one only watched) and
 * robot vacuum of `vesta-hubd --dev`, through the app's own live layer and
 * the tiles' bindings.
 *
 *   pnpm test:e2e                                        # starts a hub with cargo
 *   VESTA_HUB_URL=http://127.0.0.1:8080 pnpm test:e2e    # uses a running hub
 */
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { startHub, type RunningHub } from './hub'

import type { Command, RobotVacuumState } from '@/api/generated'
import { fetchDashboard } from '@/api/http'
import { bindWidget } from '@/live/bindings'
import { createDeviceStore, selectEndpointView, type DeviceStore } from '@/live/device-store'
import { createLiveClient, requestIdGenerator, type LiveClient } from '@/live/live-client'
import { WS_PATH } from '@/lib/ws/url'
import type { WidgetPropsMap } from '@/widgets/registry'

const LAMP = 'hall_lamp/main'
const WASHER = 'washing_machine/main'
const ROBOT = 'robot_vacuum/main'
const NO_WAKE_SIGNALS = { subscribe: () => () => undefined }

function waitUntil(check: () => boolean, timeoutMs = 10_000): Promise<void> {
  return new Promise((resolve, reject) => {
    const started = Date.now()
    const poll = () => {
      if (check()) resolve()
      else if (Date.now() - started > timeoutMs) reject(new Error('timed out'))
      else setTimeout(poll, 20)
    }
    poll()
  })
}

let hub: RunningHub | undefined
let client: LiveClient | undefined
let store: DeviceStore

function live(): LiveClient {
  if (client === undefined) throw new Error('client not started')
  return client
}

function robot(): RobotVacuumState | undefined {
  return store.getState().endpoints[ROBOT]?.capabilities.robot_vacuum?.state
}

const vacuum = (command: Extract<Command, { capability: 'robot_vacuum' }>): Command => command
const onOff = (command: Extract<Command, { capability: 'on_off' }>): Command => command
const noSend = { send: () => Promise.resolve({ ok: true as const }), fallbackLabel: 'Unknown device' }

beforeAll(async () => {
  hub = await startHub()
  store = createDeviceStore()
  client = createLiveClient({
    url: `${hub.baseUrl.replace(/^http/, 'ws')}${WS_PATH}`,
    wakeSignals: NO_WAKE_SIGNALS,
    store,
    newRequestId: requestIdGenerator('e2e-appliances'),
  })
  client.start()
  await waitUntil(() => store.getState().synced)
}, 600_000)

afterAll(async () => {
  if (client !== undefined) {
    // Leave a shared hub the way the simulator starts.
    await client.sendCommand(LAMP, onOff({ capability: 'on_off', action: 'on' }))
    await client.sendCommand(ROBOT, vacuum({ capability: 'robot_vacuum', action: 'return_to_dock' }))
    client.stop()
  }
  await hub?.stop()
})

describe('the simulated plugs and robot vacuum, end to end', () => {
  it('are on their page with tiles the editor would pick', async () => {
    if (hub === undefined) throw new Error('no hub')
    const dashboard = await fetchDashboard({ baseUrl: hub.baseUrl })
    const tiles = dashboard.pages.flatMap((page) => page.widgets)
    expect(tiles.filter((tile) => tile.type === 'outlet-control').map((tile) => tile.target)).toEqual([LAMP, WASHER])
    expect(tiles.filter((tile) => tile.type === 'robot-vacuum').map((tile) => tile.target)).toEqual([ROBOT])

    const lamp = selectEndpointView(store.getState(), LAMP)
    expect(lamp?.endpoint.type).toBe('outlet')
    expect(lamp?.endpoint.capabilities.power_meter?.features.role).toBe('load')
    const robotView = selectEndpointView(store.getState(), ROBOT)
    expect(robotView?.endpoint.type).toBe('robot_vacuum')
    expect(robotView?.endpoint.capabilities.battery?.state?.percent).toBe(100)
  })

  it('switches the lamp plug and shows what it draws', async () => {
    await expect(live().sendCommand(LAMP, onOff({ capability: 'on_off', action: 'off' }))).resolves.toEqual({ ok: true })
    await waitUntil(() => store.getState().endpoints[LAMP]?.capabilities.power_meter?.state?.power_w === 0)
    expect(store.getState().endpoints[LAMP]?.capabilities.on_off?.state?.on).toBe(false)
    await live().sendCommand(LAMP, onOff({ capability: 'on_off', action: 'on' }))
    await waitUntil(() => (store.getState().endpoints[LAMP]?.capabilities.power_meter?.state?.power_w ?? 0) > 30)
    const meter = store.getState().endpoints[LAMP]?.capabilities.power_meter?.state
    expect(meter?.voltage_v).toBe(230)
    expect(meter?.energy_wh).toBeGreaterThan(0)
  })

  it('never switches a plug that is only watched', async () => {
    const view = selectEndpointView(store.getState(), WASHER)
    expect(view?.endpoint.capabilities.on_off?.features?.read_only).toBe(true)
    const spec = bindWidget({ type: 'outlet-control', target: WASHER, col_span: 2, row_span: 1 }, view, noSend)
    const props = spec.props as WidgetPropsMap['outlet-control']
    expect(props.onToggle).toBeUndefined()
    expect(props.on).toBe(true)
    await expect(live().sendCommand(WASHER, onOff({ capability: 'on_off', action: 'off' }))).resolves.toMatchObject({
      ok: false,
      reason: 'unsupported',
    })
    expect(store.getState().endpoints[WASHER]?.capabilities.on_off?.state?.on).toBe(true)
  })

  it('starts, pauses and sends the robot home', async () => {
    await expect(live().sendCommand(ROBOT, vacuum({ capability: 'robot_vacuum', action: 'start' }))).resolves.toEqual({
      ok: true,
    })
    await waitUntil(() => robot()?.status === 'cleaning')
    await live().sendCommand(ROBOT, vacuum({ capability: 'robot_vacuum', action: 'pause' }))
    await waitUntil(() => robot()?.status === 'paused')
    await live().sendCommand(ROBOT, vacuum({ capability: 'robot_vacuum', action: 'start' }))
    await waitUntil(() => robot()?.status === 'cleaning')
    await live().sendCommand(ROBOT, vacuum({ capability: 'robot_vacuum', action: 'return_to_dock' }))
    await waitUntil(() => robot()?.status === 'returning')
    // It gets there by itself.
    await waitUntil(() => robot()?.status === 'charging', 15_000)
  })
})
