/**
 * End to end: the simulated car charger of `vesta-hubd --dev`, through the
 * app's own live layer (protocol guard, device store, optimistic client)
 * and the charger tile's binding.
 *
 *   pnpm test:e2e                                        # starts a hub with cargo
 *   VESTA_HUB_URL=http://127.0.0.1:8080 pnpm test:e2e    # uses a running hub
 */
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { startHub, type RunningHub } from './hub'

import type { Command, EvChargerState } from '@/api/generated'
import { fetchDashboard } from '@/api/http'
import { bindWidget } from '@/live/bindings'
import {
  createDeviceStore,
  selectEndpointView,
  selectRelatedEndpointViews,
  type DeviceStore,
} from '@/live/device-store'
import { createLiveClient, requestIdGenerator, type LiveClient } from '@/live/live-client'
import { WS_PATH } from '@/lib/ws/url'

const CHARGER = 'car_charger/charger'
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

function charger(): EvChargerState | undefined {
  return store.getState().endpoints[CHARGER]?.capabilities.ev_charger?.state
}

const ev = (command: Extract<Command, { capability: 'ev_charger' }>): Command => command

beforeAll(async () => {
  hub = await startHub()
  store = createDeviceStore()
  client = createLiveClient({
    url: `${hub.baseUrl.replace(/^http/, 'ws')}${WS_PATH}`,
    wakeSignals: NO_WAKE_SIGNALS,
    store,
    newRequestId: requestIdGenerator('e2e-ev'),
  })
  client.start()
  await waitUntil(() => store.getState().synced)
}, 600_000)

afterAll(async () => {
  if (client !== undefined) {
    // Leave a shared hub the way the simulator starts.
    await client.sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'resume' }))
    await client.sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_current', amps: 16 }))
    await client.sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_mode', mode: 'manual' }))
    client.stop()
  }
  await hub?.stop()
})

describe('the simulated car charger, end to end', () => {
  it('is on the Energy page and in the snapshot with its meters', async () => {
    if (hub === undefined) throw new Error('no hub')
    const dashboard = await fetchDashboard({ baseUrl: hub.baseUrl })
    const tiles = dashboard.pages.flatMap((page) => page.widgets).filter((widget) => widget.type === 'ev-charger')
    expect(tiles.map((tile) => tile.target)).toEqual([CHARGER, CHARGER])

    const view = selectEndpointView(store.getState(), CHARGER)
    expect(view?.available).toBe(true)
    expect(view?.endpoint.type).toBe('ev_charger')
    expect(view?.endpoint.capabilities.ev_charger?.features).toMatchObject({
      pause: true,
      adjustable_current: true,
      lock: true,
      schedule: true,
      modes: ['manual', 'solar', 'solar_grid'],
    })
    const related = selectRelatedEndpointViews(store.getState(), CHARGER)
    expect(related.map((other) => other.endpoint.capabilities.power_meter?.features.role)).toEqual([
      'grid',
      'house',
      'solar',
    ])

    // The tile gets the meters as readings, and the contracted power.
    const spec = bindWidget(
      { type: 'ev-charger', target: CHARGER, col_span: 4, row_span: 1 },
      view,
      { send: () => Promise.resolve({ ok: true }), fallbackLabel: 'Unknown device' },
      related,
    )
    expect(spec.props).toMatchObject({ readings: { limitW: 5_750 }, currentRange: { min: 6, max: 25 } })
  })

  it('pauses and resumes', async () => {
    await expect(live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'pause' }))).resolves.toEqual({
      ok: true,
    })
    await waitUntil(() => charger()?.status === 'paused')
    expect(charger()?.power_w).toBe(0)
    await expect(live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'resume' }))).resolves.toEqual({
      ok: true,
    })
    await waitUntil(() => charger()?.status !== 'paused')
  })

  it('sets the current within the limits the charger reports', async () => {
    await live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_current', amps: 12 }))
    await waitUntil(() => charger()?.current_a === 12)
    await live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_current', amps: 99 }))
    await waitUntil(() => charger()?.current_a === 25)
  })

  it('switches mode and the charging hours', async () => {
    await live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_mode', mode: 'solar_grid' }))
    await waitUntil(() => charger()?.mode === 'solar_grid')
    await live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_schedule', enabled: true }))
    await waitUntil(() => charger()?.schedule_enabled === true)
    await live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_schedule', enabled: false }))
    await waitUntil(() => charger()?.schedule_enabled === false)
    // A mode the charger does not offer is refused by the home.
    await expect(
      live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'set_mode', mode: 'scheduled' })),
    ).resolves.toMatchObject({ ok: false, reason: 'unsupported' })
  })

  it('locks at once, but unlocks only when confirmed', async () => {
    await live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'lock' }))
    await waitUntil(() => charger()?.locked === true)
    await expect(
      live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'unlock' })),
    ).resolves.toMatchObject({ ok: false, reason: 'rejected', rejectReason: 'confirmation_required' })
    expect(charger()?.locked).toBe(true)
    await expect(
      live().sendCommand(CHARGER, ev({ capability: 'ev_charger', action: 'unlock' }), { confirmed: true }),
    ).resolves.toEqual({ ok: true })
    await waitUntil(() => charger()?.locked === false)
  })

  it('keeps reporting by itself, never over the contracted power', async () => {
    const before = store.getState().endpoints['car_charger/house']?.capabilities.power_meter?.state
    await waitUntil(() => store.getState().endpoints['car_charger/house']?.capabilities.power_meter?.state !== before)
    const grid = store.getState().endpoints['car_charger/grid']?.capabilities.power_meter?.state
    expect(grid?.limit_w).toBe(5_750)
    expect(grid?.power_w ?? 0).toBeLessThanOrEqual(5_750)
  })
})
