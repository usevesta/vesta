import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Dashboard, ServerMessage } from '@/api/generated'
import { startDashboardSync, useDashboardStore } from '@/live/dashboard-store'
import { FakeSockets, NO_WAKE_SIGNALS } from '@/live/test-fixtures'
import { ReconnectingSocket } from '@/lib/ws/reconnecting-socket'

function dashboard(title: string): Dashboard {
  return {
    schema_version: 1,
    theme: { preset: 'glass-dark', auto_dark_mode: true, touch_scale: 'large' },
    kiosk: { screensaver_timeout_sec: 120, wake_on: [] },
    pages: [{ id: 'home', title, grid_columns: 4, widgets: [] }],
  }
}

/** A home whose dashboard the test changes. */
function fakeHome() {
  const home = { title: 'Home', revision: 'rev-1', requests: 0 }
  const fetch: typeof globalThis.fetch = () => {
    home.requests += 1
    return Promise.resolve(
      new Response(JSON.stringify(dashboard(home.title)), {
        headers: { 'Content-Type': 'application/json', ETag: `"${home.revision}"` },
      }),
    )
  }
  return { home, fetch }
}

function setup() {
  const sockets = new FakeSockets()
  const socket = new ReconnectingSocket({
    url: 'ws://localhost/api/v1/ws',
    createSocket: sockets.factory,
    wakeSignals: NO_WAKE_SIGNALS,
  })
  const listeners = new Set<(message: ServerMessage) => void>()
  const source = {
    socket,
    onMessage: (listener: (message: ServerMessage) => void) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
  const send = (message: ServerMessage) => {
    for (const listener of listeners) listener(message)
  }
  const { home, fetch } = fakeHome()
  const stop = startDashboardSync(source, { fetch })
  socket.start()
  sockets.open()
  return { home, send, stop, socket }
}

const settle = () => vi.waitFor(() => undefined)

beforeEach(() => {
  useDashboardStore.setState({ dashboard: null, revision: null, failed: false })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('dashboard sync', () => {
  it('loads the dashboard and its revision when the connection opens', async () => {
    const { home, stop, socket } = setup()
    await vi.waitFor(() => {
      expect(useDashboardStore.getState().revision).toBe('rev-1')
    })
    expect(useDashboardStore.getState().dashboard?.pages[0]?.title).toBe('Home')
    expect(home.requests).toBe(1)
    stop()
    socket.stop()
  })

  it('reloads when the home says the dashboard changed', async () => {
    const { home, send, stop, socket } = setup()
    await vi.waitFor(() => {
      expect(useDashboardStore.getState().revision).toBe('rev-1')
    })

    home.title = 'Inicio'
    home.revision = 'rev-2'
    send({ type: 'dashboard_changed', revision: 'rev-2' })
    await vi.waitFor(() => {
      expect(useDashboardStore.getState().revision).toBe('rev-2')
    })
    expect(useDashboardStore.getState().dashboard?.pages[0]?.title).toBe('Inicio')
    expect(home.requests).toBe(2)

    // Already showing that revision (e.g. this screen saved it): no reload.
    send({ type: 'dashboard_changed', revision: 'rev-2' })
    // Other messages are not about the dashboard.
    send({ type: 'availability', device_id: 'lamp', online: false })
    await settle()
    expect(home.requests).toBe(2)

    stop()
    send({ type: 'dashboard_changed', revision: 'rev-3' })
    await settle()
    expect(home.requests).toBe(2)
    socket.stop()
  })
})
