import { lazy, Suspense, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { Dashboard, Widget } from '@/api/generated'
import { HeaderButton } from '@/app/HeaderButton'
import { DashboardGrid, DashboardGridItem } from '@/components/dashboard/DashboardGrid'
import { AppShell } from '@/components/shell/AppShell'
import { EmptyState } from '@/components/shell/EmptyState'
import type { ShellPage } from '@/components/shell/PageNav'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { dismissConflictNotice, startEditing, useEditorStore, type SaveDashboard } from '@/editor/editor-store'
import type { HomeItem } from '@/editor/model'
import { isIconName } from '@/lib/icons'

/** Edit mode is loaded on first use: most screens never need it. */
const EditScreen = lazy(() => import('@/editor/EditScreen').then((module) => ({ default: module.EditScreen })))

export interface DashboardScreenProps {
  /** The saved dashboard; null while it is not loaded yet. */
  dashboard: Dashboard | null
  /** Its revision, to save edits against. */
  revision: string | null
  /** Shown while there is no dashboard yet (waiting, or loading failed). */
  placeholder: ReactNode
  /** A live (or demo) tile for one widget. */
  renderWidget: (widget: Widget) => ReactNode
  /** Everything in the home a tile can show, for "Add" and for naming tiles. */
  items: readonly HomeItem[]
  /** `items` reflects the home (otherwise missing things are not called missing). */
  itemsReady: boolean
  save: SaveDashboard
  /** Above the content (connection state). */
  banner?: ReactNode
  /** Extra header items before the edit button (demo badge). */
  headerExtra?: ReactNode
}

/** "Someone else saved first": shown once the edit screen has closed. */
function ConflictNotice() {
  const { t } = useTranslation()
  const open = useEditorStore((s) => s.conflictNotice)
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) dismissConflictNotice()
      }}
    >
      <AlertDialogContent className="gap-6 p-6 data-[size=default]:max-w-md data-[size=default]:sm:max-w-md">
        <AlertDialogHeader>
          <AlertDialogTitle className="text-2xl">{t('editor.conflictTitle')}</AlertDialogTitle>
          <AlertDialogDescription className="text-base">{t('editor.conflictBody')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter className="-mx-6 -mb-6 p-6">
          <AlertDialogAction className="h-auto min-h-14 flex-1 text-lg">{t('editor.conflictOk')}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/**
 * A home's pages and tiles, and the way into the screen editor. While
 * editing, the screen shows the draft; everything else (kiosk, other
 * screens) keeps the saved dashboard until "Done".
 */
export function DashboardScreen({
  dashboard,
  revision,
  placeholder,
  renderWidget,
  items,
  itemsReady,
  save,
  banner,
  headerExtra,
}: DashboardScreenProps) {
  const { t } = useTranslation()
  const session = useEditorStore((s) => s.session)
  const [selectedPageId, setSelectedPageId] = useState<string | null>(null)

  const pages = useMemo(() => dashboard?.pages ?? [], [dashboard])
  const page = pages.find((candidate) => candidate.id === selectedPageId) ?? pages[0]
  const shellPages = useMemo<ShellPage[]>(
    () => pages.map(({ id, title, icon }) => ({ id, title, icon: icon ?? 'home' })),
    [pages],
  )

  let content: ReactNode
  if (dashboard === null) {
    content = placeholder
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
            {renderWidget(widget)}
          </DashboardGridItem>
        ))}
      </DashboardGrid>
    )
  }

  const view = (
    <AppShell
      pages={shellPages}
      activePageId={page?.id ?? null}
      onSelectPage={setSelectedPageId}
      banner={banner}
      headerExtra={
        <>
          {headerExtra}
          {dashboard !== null && (
            <HeaderButton
              icon="edit"
              label={t('editor.edit')}
              variant="ghost"
              onClick={() => {
                startEditing(dashboard, revision)
              }}
            />
          )}
        </>
      }
    >
      {content}
    </AppShell>
  )

  return (
    <>
      {session === null ? (
        view
      ) : (
        <Suspense fallback={view}>
          <EditScreen
            session={session}
            revision={revision}
            items={items}
            itemsReady={itemsReady}
            save={save}
            banner={banner}
            selectedPageId={selectedPageId ?? page?.id ?? null}
            onSelectPage={setSelectedPageId}
          />
        </Suspense>
      )}
      <ConflictNotice />
    </>
  )
}
