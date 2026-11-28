/**
 * End-to-end: the screen editor's model and API layer against a real
 * `vesta-hubd --dev`. Edits a dashboard like a person would (add a page,
 * add, move, resize, rename and remove tiles), saves it, and checks that
 * other screens hear about it, that stale saves are refused and that the
 * home stored exactly what was sent.
 *
 * Runs against its own hub (edits there stay in memory); with
 * `VESTA_HUB_URL` the original dashboard is put back at the end.
 */
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { startHub, type RunningHub } from './hub'

import { fetchVersionedDashboard, saveDashboard } from '@/api/dashboard-api'
import type { Dashboard, ServerMessage } from '@/api/generated'
import { applyEdit, fromDraft, newPageId, toDraft, widgetForItem, type Draft, type EditAction } from '@/editor/model'
import { startDashboardSync, useDashboardStore } from '@/live/dashboard-store'
import { createDeviceStore, type DeviceStore } from '@/live/device-store'
import { homeItems } from '@/live/editor'
import { createLiveClient, type LiveClient } from '@/live/live-client'
import { WS_PATH } from '@/lib/ws/url'

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
let stopSync: (() => void) | undefined
let store: DeviceStore
let original: Dashboard | undefined
let originalRevision: string | null = null
const messages: ServerMessage[] = []

function baseUrl(): string {
  if (hub === undefined) throw new Error('hub not started')
  return hub.baseUrl
}

function changes(): string[] {
  return messages.flatMap((message) => (message.type === 'dashboard_changed' ? [message.revision] : []))
}

function edit(draft: Draft, ...actions: EditAction[]): Draft {
  return actions.reduce(applyEdit, draft)
}

beforeAll(async () => {
  hub = await startHub()
  const loaded = await fetchVersionedDashboard({ baseUrl: baseUrl() })
  original = loaded.dashboard
  originalRevision = loaded.revision

  store = createDeviceStore()
  client = createLiveClient({
    url: `${baseUrl().replace(/^http/, 'ws')}${WS_PATH}`,
    wakeSignals: NO_WAKE_SIGNALS,
    store,
  })
  client.onMessage((message) => messages.push(message))
  // A screen that is not being edited: it should follow every save.
  stopSync = startDashboardSync(client, { baseUrl: baseUrl() })
  client.start()
  await waitUntil(() => store.getState().synced && useDashboardStore.getState().revision !== null)
}, 600_000)

afterAll(async () => {
  stopSync?.()
  client?.stop()
  if (original !== undefined && hub !== undefined) {
    // Leave a shared hub the way it was.
    const current = await fetchVersionedDashboard({ baseUrl: hub.baseUrl })
    await saveDashboard(original, current.revision, { baseUrl: hub.baseUrl })
  }
  await hub?.stop()
})

describe('editing the screen against vesta-hubd --dev', () => {
  it('serves the dashboard with a revision', () => {
    expect(originalRevision).toMatch(/^[0-9a-f]{16}$/)
    expect(useDashboardStore.getState().revision).toBe(originalRevision)
  })

  it('saves an edit made like a person makes it, and every screen follows', async () => {
    if (original === undefined) throw new Error('no dashboard')
    const items = homeItems(store.getState())
    const blind = items.find((item) => item.type === 'window_covering')
    const fanLight = items.find((item) => item.partOf !== undefined && item.type === 'light')
    expect(blind?.name).toBe('Bedroom Blind')
    expect(fanLight?.partOf).toBe('Living Room Fan')
    if (blind === undefined || fanLight === undefined) throw new Error('missing things')

    let draft = toDraft(original)
    const home = draft.pages[0]
    if (home === undefined) throw new Error('no page')
    const pageId = newPageId(draft)
    const blindTile = widgetForItem(blind, 4)
    const lightTile = widgetForItem(fanLight, 4)
    if (blindTile === null || lightTile === null) throw new Error('no tile for them')
    const [first, second, third] = home.widgets.map(({ key }) => key)
    if (first === undefined || second === undefined || third === undefined) throw new Error('too few tiles')

    draft = edit(
      draft,
      { type: 'addPage', id: pageId, title: 'Dormitorio', icon: 'bedroom' },
      { type: 'addWidget', pageId, widget: blindTile },
      { type: 'addWidget', pageId, widget: lightTile },
      { type: 'moveWidget', pageId: home.id, key: first, by: 1 },
      { type: 'resizeWidget', pageId: home.id, key: second, size: 'full' },
      { type: 'renameWidget', pageId: home.id, key: second, label: 'Puerta de la calle' },
      { type: 'removeWidget', pageId: home.id, key: third },
      // From the end to the front, one step at a time.
      ...original.pages.map(() => ({ type: 'movePage' as const, pageId, by: -1 as const })),
    )
    const edited = fromDraft(draft)

    const result = await saveDashboard(edited, originalRevision, { baseUrl: baseUrl() })
    if (result.status !== 'saved') throw new Error(`not saved: ${result.status}`)
    expect(result.dashboard).toEqual(edited)
    expect(result.revision).not.toBe(originalRevision)

    // Every session hears about it, and a screen that is not being edited reloads.
    await waitUntil(() => changes().includes(result.revision ?? ''))
    await waitUntil(() => useDashboardStore.getState().revision === result.revision)
    expect(useDashboardStore.getState().dashboard).toEqual(edited)

    const reloaded = await fetchVersionedDashboard({ baseUrl: baseUrl() })
    expect(reloaded).toEqual({ dashboard: edited, revision: result.revision })
    expect(reloaded.dashboard.pages.map((page) => page.title)).toEqual(['Dormitorio', ...original.pages.map((page) => page.title)])
    expect(reloaded.dashboard.pages[0]?.widgets.map((widget) => widget.type)).toEqual(['cover-control', 'light-control'])
  })

  it('refuses a save based on an old version (someone else saved first)', async () => {
    if (original === undefined) throw new Error('no dashboard')
    const current = await fetchVersionedDashboard({ baseUrl: baseUrl() })
    const mine = fromDraft(edit(toDraft(original), { type: 'removePage', pageId: original.pages[0]?.id ?? '' }))
    await expect(saveDashboard(mine, originalRevision, { baseUrl: baseUrl() })).resolves.toEqual({
      status: 'conflict',
      revision: current.revision,
    })
    // Nothing changed.
    await expect(fetchVersionedDashboard({ baseUrl: baseUrl() })).resolves.toEqual(current)
  })

  it('refuses saves without a revision or that are not a valid dashboard', async () => {
    const current = await fetchVersionedDashboard({ baseUrl: baseUrl() })
    await expect(saveDashboard(current.dashboard, null, { baseUrl: baseUrl() })).resolves.toEqual({
      status: 'rejected',
      httpStatus: 428,
    })
    const [page] = current.dashboard.pages
    if (page === undefined) throw new Error('no page')
    const duplicate = { ...current.dashboard, pages: [page, page] }
    await expect(saveDashboard(duplicate, current.revision, { baseUrl: baseUrl() })).resolves.toEqual({
      status: 'rejected',
      httpStatus: 422,
    })
    // A thing that does not exist (any more) is allowed: it shows as unavailable.
    const unknown = {
      ...current.dashboard,
      pages: [{ ...page, widgets: [{ type: 'light-control', target: 'attic_light/main', col_span: 1, row_span: 1 }] }],
    }
    const saved = await saveDashboard(unknown, current.revision, { baseUrl: baseUrl() })
    expect(saved.status).toBe('saved')
  })

  it('puts the original back with the original revision (content-based)', async () => {
    if (original === undefined) throw new Error('no dashboard')
    const current = await fetchVersionedDashboard({ baseUrl: baseUrl() })
    const restored = await saveDashboard(original, current.revision, { baseUrl: baseUrl() })
    expect(restored).toEqual({ status: 'saved', dashboard: original, revision: originalRevision })
    await waitUntil(() => useDashboardStore.getState().revision === originalRevision)
  })
})
