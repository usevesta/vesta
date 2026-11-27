/**
 * The app's single connection to the home. Everything live (widgets, the
 * connection banner, the doorbell, the screensaver) goes through this one
 * client; nothing else opens a WebSocket.
 */
import i18n from 'i18next'
import { toast } from 'sonner'

import { commandErrorKey } from '@/live/command-errors'
import { startDashboardSync } from '@/live/dashboard-store'
import { useDeviceStore } from '@/live/device-store'
import { createLiveClient, type CommandFailure, type LiveClient } from '@/live/live-client'
import { bindConnectionStore } from '@/lib/ws/connection-store'
import { sameOriginSocketUrl } from '@/lib/ws/url'

function showCommandFailure(failure: CommandFailure): void {
  const name = failure.label ?? i18n.t('widget.unknownDevice')
  // One message per device: repeated taps replace it instead of stacking.
  toast.error(i18n.t(commandErrorKey(failure), { name }), { id: `command:${failure.target}` })
}

let client: LiveClient | null = null

/** The app-wide client (created on first use). */
export function liveClient(): LiveClient {
  client ??= createLiveClient({
    url: () => sameOriginSocketUrl(window.location),
    store: useDeviceStore,
    onCommandFailed: showCommandFailure,
  })
  return client
}

/** Connect, keep the dashboard and connection state in sync. Returns a stop function. */
export function startLive(): () => void {
  const live = liveClient()
  const unbindConnection = bindConnectionStore(live.socket)
  const stopDashboard = startDashboardSync(live.socket)
  live.start()
  return () => {
    stopDashboard()
    unbindConnection()
    live.stop()
  }
}
