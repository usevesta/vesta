import { create } from 'zustand'

import type { Dashboard } from '@/api/generated'
import { fetchDashboard, type RequestOptions } from '@/api/http'
import type { ReconnectingSocket } from '@/lib/ws/reconnecting-socket'
import { isThemePreset } from '@/theme/theme'
import { useThemeStore } from '@/theme/theme-store'

export interface DashboardState {
  /** Last dashboard the home sent; kept while the connection is down. */
  dashboard: Dashboard | null
  /** Loading failed and there is nothing to show yet. */
  failed: boolean
}

export const useDashboardStore = create<DashboardState>()(() => ({ dashboard: null, failed: false }))

/** Wait this long before asking again after a failed load. */
const RETRY_MS = 5_000

function applyDashboard(dashboard: Dashboard): void {
  const current = useDashboardStore.getState().dashboard
  // Unchanged after a reconnect: keep the same object, so the screensaver
  // and doorbell timers are not restarted.
  if (current !== null && JSON.stringify(current) === JSON.stringify(dashboard)) {
    useDashboardStore.setState({ failed: false })
  } else {
    useDashboardStore.setState({ dashboard, failed: false })
  }
  const { preset } = dashboard.theme
  useThemeStore.getState().setHomePreset(isThemePreset(preset) ? preset : null)
}

/**
 * Load the dashboard whenever the connection (re)opens, so changes made
 * elsewhere show up after a reconnect. Returns a stop function.
 */
export function startDashboardSync(socket: ReconnectingSocket, options: Omit<RequestOptions, 'signal'> = {}): () => void {
  let controller: AbortController | null = null
  let retryTimer: ReturnType<typeof setTimeout> | null = null

  const load = () => {
    if (retryTimer !== null) clearTimeout(retryTimer)
    retryTimer = null
    controller?.abort()
    const current = new AbortController()
    controller = current
    fetchDashboard({ ...options, signal: current.signal }).then(
      (dashboard) => {
        if (!current.signal.aborted) applyDashboard(dashboard)
      },
      () => {
        if (current.signal.aborted) return
        // Keep showing the last good dashboard, if any, and try again.
        useDashboardStore.setState((state) => ({ failed: state.dashboard === null }))
        if (socket.snapshot.status === 'open') retryTimer = setTimeout(load, RETRY_MS)
      },
    )
  }

  const unsubscribe = socket.onOpen(load)
  if (socket.snapshot.status === 'open') load()
  return () => {
    unsubscribe()
    controller?.abort()
    if (retryTimer !== null) clearTimeout(retryTimer)
  }
}
