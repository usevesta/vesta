import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import type { Dashboard } from '@/api/generated'
import { DashboardScreen } from '@/app/DashboardScreen'
import { ConnectionBanner } from '@/components/shell/ConnectionBanner'
import { EmptyState } from '@/components/shell/EmptyState'
import { useDashboardStore } from '@/live/dashboard-store'
import { saveLiveDashboard, useHomeItems } from '@/live/editor'
import { DoorbellOverlayView } from '@/live/kiosk/DoorbellOverlayView'
import { startDoorbell, startScreensaver } from '@/live/kiosk/kiosk'
import { Screensaver } from '@/live/kiosk/Screensaver'
import { startScreenControl } from '@/live/kiosk/screen'
import { liveClient, startLive } from '@/live/live'
import { LiveWidget } from '@/live/LiveWidget'
import { useConnectionStore } from '@/lib/ws/connection-store'
import { platform } from '@/platform'

/**
 * Screensaver and doorbell overlay, configured by the dashboard. The screen
 * stays awake and dims with the screensaver where the device allows it.
 */
function Kiosk({ dashboard }: { dashboard: Dashboard }) {
  const { kiosk, doorbell_overlay: overlay } = dashboard
  useEffect(() => startScreenControl(platform()), [])
  useEffect(() => startScreensaver(kiosk, liveClient()), [kiosk])
  useEffect(() => (overlay === undefined ? undefined : startDoorbell(overlay, liveClient())), [overlay])
  return (
    <>
      <Screensaver />
      {overlay !== undefined && <DoorbellOverlayView overlay={overlay} />}
    </>
  )
}

/**
 * Live mode: one connection to the home (see `src/live/live.ts`); pages
 * and widgets come from the home's dashboard, state from its devices.
 * The screen editor saves back to the home.
 */
export function LiveApp() {
  const { t } = useTranslation()
  const dashboard = useDashboardStore((s) => s.dashboard)
  const revision = useDashboardStore((s) => s.revision)
  const failed = useDashboardStore((s) => s.failed)
  const status = useConnectionStore((s) => s.status)
  const { items, ready } = useHomeItems()

  useEffect(() => startLive(), [])

  const placeholder = failed ? (
    <EmptyState icon="warning" title={t('dashboard.loadFailedTitle')} body={t('dashboard.loadFailedBody')} />
  ) : (
    <EmptyState
      icon={status === 'open' ? 'loading' : 'connection_lost'}
      title={t('connection.waitingTitle')}
      body={t('connection.waitingBody')}
    />
  )

  return (
    <>
      <DashboardScreen
        dashboard={dashboard}
        revision={revision}
        placeholder={placeholder}
        renderWidget={(widget) => <LiveWidget widget={widget} />}
        items={items}
        itemsReady={ready}
        save={saveLiveDashboard}
        banner={
          <ConnectionBanner
            onRetry={() => {
              liveClient().socket.reconnectNow()
            }}
          />
        }
      />
      {dashboard !== null && <Kiosk dashboard={dashboard} />}
    </>
  )
}
