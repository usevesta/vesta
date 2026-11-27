import { create } from 'zustand'

import { fetchVersionedDashboard } from '@/api/dashboard-api'
import type { Dashboard, ServerMessage } from '@/api/generated'
import type { RequestOptions } from '@/api/http'
import type { ReconnectingSocket } from '@/lib/ws/reconnecting-socket'
import { isThemePreset } from '@/theme/theme'
import { useThemeStore } from '@/theme/theme-store'

export interface DashboardState {
  /** Last dashboard the home sent; kept while the connection is down. */
  dashboard: Dashboard | null
  /** Its revision (the `ETag`), to save edits against; null until loaded. */
  revision: string | null
  /** Loading failed and there is nothing to show yet. */
  failed: boolean
}

export const useDashboardStore = create<DashboardState>()(() => ({ dashboard: null, revision: null, failed: false }))

/** Wait this long before asking again after a failed load. */
const RETRY_MS = 5_000

/** Show `dashboard` (loaded, or just saved from this screen). */
export function setDashboard(dashboard: Dashboard, revision: string | null): void {
  const current = useDashboardStore.getState().dashboard
  // Unchanged after a reconnect: keep the same object, so the screensaver
  // and doorbell timers are not restarted.
  if (current !== null && JSON.stringify(current) === JSON.stringify(dashboard)) {
    useDashboardStore.setState({ revision, failed: false })
  } else {
    useDashboardStore.setState({ dashboard, revision, failed: false })
  }
  const { preset } = dashboard.theme
  useThemeStore.getState().setHomePreset(isThemePreset(preset) ? preset : null)
}

/** Where the dashboard sync listens: the app's connection to the home. */
export interface DashboardSyncSource {
  readonly socket: ReconnectingSocket
  /** Decoded messages from the home (`dashboard_changed`). */
  onMessage: (listener: (message: ServerMessage) => void) => () => void
}

/**
 * Load the dashboard whenever the connection (re)opens, so changes made
 * elsewhere show up after a reconnect, and whenever the home says it
 * changed (unless this screen already shows that revision, e.g. because
 * it saved it). Returns a stop function.
 */
export function startDashboardSync(
  source: DashboardSyncSource,
  options: Omit<RequestOptions, 'signal'> = {},
): () => void {
  const { socket } = source
  let controller: AbortController | null = null
  let retryTimer: ReturnType<typeof setTimeout> | null = null

  const load = () => {
    if (retryTimer !== null) clearTimeout(retryTimer)
    retryTimer = null
    controller?.abort()
    const current = new AbortController()
    controller = current
    fetchVersionedDashboard({ ...options, signal: current.signal }).then(
      ({ dashboard, revision }) => {
        if (!current.signal.aborted) setDashboard(dashboard, revision)
      },
      () => {
        if (current.signal.aborted) return
        // Keep showing the last good dashboard, if any, and try again.
        useDashboardStore.setState((state) => ({ failed: state.dashboard === null }))
        if (socket.snapshot.status === 'open') retryTimer = setTimeout(load, RETRY_MS)
      },
    )
  }

  const unsubscribeOpen = socket.onOpen(load)
  const unsubscribeMessages = source.onMessage((message) => {
    if (message.type === 'dashboard_changed' && message.revision !== useDashboardStore.getState().revision) load()
  })
  if (socket.snapshot.status === 'open') load()
  return () => {
    unsubscribeOpen()
    unsubscribeMessages()
    controller?.abort()
    if (retryTimer !== null) clearTimeout(retryTimer)
  }
}

/** Load the latest dashboard once (after a save was refused). Never throws. */
export async function reloadDashboard(options: Omit<RequestOptions, 'signal'> = {}): Promise<void> {
  try {
    const { dashboard, revision } = await fetchVersionedDashboard(options)
    setDashboard(dashboard, revision)
  } catch {
    // The next reconnect or `dashboard_changed` loads it.
  }
}
