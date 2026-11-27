/**
 * End-to-end smoke test: the app's own API layer (protocol guard, device
 * store, optimistic live client) against a real `vesta-hubd --dev`.
 *
 *   pnpm test:e2e                                   # starts a hub with cargo
 *   VESTA_HUB_URL=http://127.0.0.1:8080 pnpm test:e2e   # uses a running hub
 */
import { fileURLToPath } from 'node:url'

import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { createServer, type ViteDevServer } from 'vite'

import { freePort, startHub, type RunningHub } from './hub'

import type { Command, CommandResult, ServerMessage, StateChange } from '@/api/generated'
import { fetchDashboard } from '@/api/http'
import { decodeServerMessage } from '@/api/protocol'
import { createDeviceStore, selectEndpointView, type DeviceStore } from '@/live/device-store'
import { createLiveClient, requestIdGenerator, type CommandFailure, type LiveClient } from '@/live/live-client'
import { WS_PATH } from '@/lib/ws/url'
import { isWidgetType } from '@/widgets/registry'

const FAN_LIGHT = 'living_room_fan/light'
const FRONT_DOOR = 'front_door_strike/main'
const NO_WAKE_SIGNALS = { subscribe: () => () => undefined }

function waitUntil(check: () => boolean, timeoutMs = 10_000): Promise<void> {
  return new Promise((resolve, reject) => {
    const started = Date.now()
    const poll = () => {
      if (check()) {
        resolve()
      } else if (Date.now() - started > timeoutMs) {
        reject(new Error('timed out'))
      } else {
        setTimeout(poll, 20)
      }
    }
    poll()
  })
}

let hub: RunningHub | undefined
let client: LiveClient | undefined
let store: DeviceStore
const messages: ServerMessage[] = []
const failures: CommandFailure[] = []

function running(): RunningHub {
  if (hub === undefined) throw new Error('hub not started')
  return hub
}

function live(): LiveClient {
  if (client === undefined) throw new Error('client not started')
  return client
}

function resultFor(requestId: string): CommandResult | undefined {
  return messages.find(
    (message): message is Extract<ServerMessage, { type: 'command_result' }> =>
      message.type === 'command_result' && message.request_id === requestId,
  )
}

function fanLightOn(): boolean | undefined {
  return store.getState().endpoints[FAN_LIGHT]?.capabilities.on_off?.state?.on
}

beforeAll(async () => {
  hub = await startHub()
}, 600_000)

afterAll(async () => {
  client?.stop()
  await hub?.stop()
})

describe('vesta-hubd --dev, end to end', () => {
  it('serves a dashboard every widget of which this screen can render', async () => {
    const dashboard = await fetchDashboard({ baseUrl: running().baseUrl })
    const widgets = dashboard.pages.flatMap((page) => page.widgets)
    expect(widgets.map((widget) => widget.target)).toContain(FAN_LIGHT)
    expect(widgets.every((widget) => isWidgetType(widget.type))).toBe(true)
    expect(dashboard.doorbell_overlay?.doorbell_target).toBe('front_doorbell/main')
  })

  it('connects over the WebSocket and receives a snapshot', async () => {
    store = createDeviceStore()
    client = createLiveClient({
      url: `${running().baseUrl.replace(/^http/, 'ws')}${WS_PATH}`,
      wakeSignals: NO_WAKE_SIGNALS,
      store,
      newRequestId: requestIdGenerator('e2e'),
      onCommandFailed: (failure) => failures.push(failure),
    })
    client.onMessage((message) => messages.push(message))
    client.start()
    await waitUntil(() => store.getState().synced)
    expect(messages[0]?.type).toBe('snapshot')
    expect(fanLightOn()).toBeTypeOf('boolean')
    expect(selectEndpointView(store.getState(), FAN_LIGHT)?.available).toBe(true)
  })

  it('toggles the fan light: pending at once, then the new state and command_result', async () => {
    const before = fanLightOn()
    const command: Command = { capability: 'on_off', action: before === true ? 'off' : 'on' }
    const outcome = live().sendCommand(FAN_LIGHT, command, { label: 'Fan Light' })

    // Optimistic: shown at once, marked pending, confirmed state untouched.
    expect(selectEndpointView(store.getState(), FAN_LIGHT)).toMatchObject({
      pending: true,
      endpoint: { capabilities: { on_off: { state: { on: before !== true } } } },
    })
    expect(fanLightOn()).toBe(before)

    await expect(outcome).resolves.toEqual({ ok: true })
    await waitUntil(() => selectEndpointView(store.getState(), FAN_LIGHT)?.pending === false)
    expect(fanLightOn()).toBe(before !== true)

    expect(resultFor('c-e2e-1')).toEqual({ type: 'command_result', request_id: 'c-e2e-1', ok: true })
    const state = messages.find(
      (message): message is Extract<ServerMessage, { type: 'state' }> & StateChange =>
        message.type === 'state' && message.target === FAN_LIGHT && message.capability === 'on_off',
    )
    expect(state?.state).toEqual({ on: before !== true })

    // Put it back the way it was.
    await expect(
      live().sendCommand(FAN_LIGHT, { capability: 'on_off', action: before === true ? 'on' : 'off' }),
    ).resolves.toEqual({ ok: true })
    await waitUntil(() => fanLightOn() === before)
  })

  it('refuses to open the door without confirmation, and opens it with', async () => {
    await expect(live().sendCommand(FRONT_DOOR, { capability: 'trigger', action: 'fire' })).resolves.toMatchObject({
      ok: false,
      reason: 'rejected',
      rejectReason: 'confirmation_required',
    })
    await expect(
      live().sendCommand(FRONT_DOOR, { capability: 'trigger', action: 'fire' }, { confirmed: true }),
    ).resolves.toEqual({ ok: true })
    expect(selectEndpointView(store.getState(), FRONT_DOOR)?.pending).toBe(false)
  })

  it('answers commands the home cannot run with unsupported', async () => {
    await expect(live().sendCommand('attic/main', { capability: 'on_off', action: 'on' })).resolves.toMatchObject({
      ok: false,
      reason: 'unsupported',
    })
    await expect(
      live().sendCommand('living_room_fan/fan', { capability: 'fan', action: 'set_speed', speed: 7 }),
    ).resolves.toMatchObject({ ok: false, reason: 'unsupported' })
    expect(failures.map((failure) => failure.reason)).toEqual(['rejected', 'unsupported', 'unsupported'])
  })
})

describe('through the Vite dev proxy (what the browser uses)', () => {
  let vite: ViteDevServer | undefined
  let origin = ''

  beforeAll(async () => {
    const port = await freePort()
    process.env.VESTA_HUB_ORIGIN = running().baseUrl
    vite = await createServer({
      configFile: fileURLToPath(new URL('../vite.config.ts', import.meta.url)),
      logLevel: 'silent',
      server: { port, strictPort: true, host: '127.0.0.1' },
    })
    await vite.listen()
    origin = `http://localhost:${String(port)}`
  }, 60_000)

  afterAll(async () => {
    await vite?.close()
  })

  it('passes the local-only guard for page requests', async () => {
    const proxied = origin.replace('localhost', '127.0.0.1')
    const response = await fetch(`${proxied}/api/v1/dashboard`, { headers: { Origin: origin } })
    expect(response.status).toBe(200)
    const foreign = await fetch(`${proxied}/api/v1/dashboard`, { headers: { Origin: 'http://example.com' } })
    expect(foreign.status).toBe(403)
  })

  it('proxies the WebSocket', async () => {
    const socket = new WebSocket(`${origin.replace('http', 'ws').replace('localhost', '127.0.0.1')}${WS_PATH}`)
    const first = await new Promise<ServerMessage | null>((resolve, reject) => {
      socket.onmessage = (event: MessageEvent<unknown>) => {
        resolve(typeof event.data === 'string' ? decodeServerMessage(JSON.parse(event.data)) : null)
      }
      socket.onerror = () => {
        reject(new Error('WebSocket through the proxy failed'))
      }
    })
    socket.close()
    expect(first?.type).toBe('snapshot')
  })
})
