import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { Dashboard } from '@/api/generated'
import { DashboardGrid, DashboardGridItem } from '@/components/dashboard/DashboardGrid'
import { AppShell } from '@/components/shell/AppShell'
import { ConnectionBanner } from '@/components/shell/ConnectionBanner'
import { EmptyState } from '@/components/shell/EmptyState'
import type { ShellPage } from '@/components/shell/PageNav'
import { useDashboardStore } from '@/live/dashboard-store'
import { DoorbellOverlayView } from '@/live/kiosk/DoorbellOverlayView'
import { startDoorbell, startScreensaver } from '@/live/kiosk/kiosk'
import { Screensaver } from '@/live/kiosk/Screensaver'
import { liveClient, startLive } from '@/live/live'
import { LiveWidget } from '@/live/LiveWidget'
import { isIconName } from '@/lib/icons'
import { useConnectionStore } from '@/lib/ws/connection-store'

/** Screensaver and doorbell overlay, configured by the dashboard. */
function Kiosk({ dashboard }: { dashboard: Dashboard }) {
  const { kiosk, doorbell_overlay: overlay } = dashboard
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
 */
export function LiveApp() {
  const { t } = useTranslation()
  const dashboard = useDashboardStore((s) => s.dashboard)
  const failed = useDashboardStore((s) => s.failed)
  const status = useConnectionStore((s) => s.status)
  const [selectedPageId, setSelectedPageId] = useState<string | null>(null)

  useEffect(() => startLive(), [])

  const pages = useMemo(() => dashboard?.pages ?? [], [dashboard])
  const page = pages.find((candidate) => candidate.id === selectedPageId) ?? pages[0]
  const shellPages = useMemo<ShellPage[]>(
    () => pages.map(({ id, title, icon }) => ({ id, title, icon: icon ?? 'home' })),
    [pages],
  )

  let content: ReactNode
  if (dashboard === null) {
    content = failed ? (
      <EmptyState icon="warning" title={t('dashboard.loadFailedTitle')} body={t('dashboard.loadFailedBody')} />
    ) : (
      <EmptyState
        icon={status === 'open' ? 'loading' : 'connection_lost'}
        title={t('connection.waitingTitle')}
        body={t('connection.waitingBody')}
      />
    )
  } else if (page === undefined) {
    content = <EmptyState icon="home" title={t('dashboard.noPagesTitle')} body={t('dashboard.noPagesBody')} />
  } else if (page.widgets.length === 0) {
    const icon = page.icon !== undefined && isIconName(page.icon) ? page.icon : 'home'
    content = <EmptyState icon={icon} title={t('dashboard.emptyPageTitle')} body={t('dashboard.emptyPageBody')} />
  } else {
    content = (
      <DashboardGrid columns={page.grid_columns}>
        {page.widgets.map((widget, index) => (
          // Widgets have no id; their position on the page is stable.
          <DashboardGridItem key={`${page.id}:${String(index)}`} colSpan={widget.col_span} rowSpan={widget.row_span}>
            <LiveWidget widget={widget} />
          </DashboardGridItem>
        ))}
      </DashboardGrid>
    )
  }

  return (
    <>
      <AppShell
        pages={shellPages}
        activePageId={page?.id ?? null}
        onSelectPage={setSelectedPageId}
        banner={
          <ConnectionBanner
            onRetry={() => {
              liveClient().socket.reconnectNow()
            }}
          />
        }
      >
        {content}
      </AppShell>
      {dashboard !== null && <Kiosk dashboard={dashboard} />}
    </>
  )
}
