/**
 * The screen in edit mode. Loaded only when someone taps "Edit", so the
 * wall panel's everyday screen stays small and quick to start.
 */
import { useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import type { Target } from '@/api/generated'
import { HeaderButton } from '@/app/HeaderButton'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DashboardGrid, DashboardGridItem } from '@/components/dashboard/DashboardGrid'
import { Icon } from '@/components/Icon'
import { AppShell } from '@/components/shell/AppShell'
import { EmptyState } from '@/components/shell/EmptyState'
import type { ShellPage } from '@/components/shell/PageNav'
import { Button } from '@/components/ui/button'
import { AddSheet } from '@/editor/AddSheet'
import { useDescribeTile } from '@/editor/describe'
import {
  cancelEditing,
  editDraft,
  finishEditing,
  hasUnsavedChanges,
  useEditorStore,
  type EditSession,
  type SaveDashboard,
} from '@/editor/editor-store'
import { EditTile } from '@/editor/EditTile'
import { newPageId, widgetLabel, type DraftPage, type DraftWidget, type HomeItem } from '@/editor/model'
import { PageNameDialog, TileNameDialog } from '@/editor/NameDialogs'
import { PagesDialog } from '@/editor/PagesDialog'

export interface EditScreenProps {
  session: EditSession
  /** Revision of the saved dashboard now; differs from the session's when someone else saved. */
  revision: string | null
  items: readonly HomeItem[]
  itemsReady: boolean
  save: SaveDashboard
  banner?: ReactNode
  selectedPageId: string | null
  onSelectPage: (pageId: string) => void
}

type Dialog =
  | { readonly kind: 'none' }
  | { readonly kind: 'addPage' }
  | { readonly kind: 'renamePage'; readonly page: DraftPage }
  | { readonly kind: 'removePage'; readonly page: DraftPage }
  | { readonly kind: 'renameWidget'; readonly pageId: string; readonly entry: DraftWidget }
  | { readonly kind: 'discard' }

const NO_DIALOG: Dialog = { kind: 'none' }

/**
 * Edit mode: the draft's pages with big controls on every tile, plus
 * "Add", "Pages", "Cancel" and "Done" in the header. Nothing is saved
 * before "Done"; other screens keep showing the saved dashboard.
 */
export function EditScreen({
  session,
  revision,
  items,
  itemsReady,
  save,
  banner,
  selectedPageId,
  onSelectPage,
}: EditScreenProps) {
  const { t } = useTranslation()
  const saving = useEditorStore((s) => s.saving)
  const describe = useDescribeTile()
  const [addOpen, setAddOpen] = useState(false)
  const [pagesOpen, setPagesOpen] = useState(false)
  const [dialog, setDialog] = useState<Dialog>(NO_DIALOG)

  const { pages } = session.draft
  const page = pages.find((candidate) => candidate.id === selectedPageId) ?? pages[0]
  const shellPages = useMemo<ShellPage[]>(
    () => pages.map(({ id, title, icon }) => ({ id, title, icon: icon ?? 'home' })),
    [pages],
  )
  const itemsByTarget = useMemo(() => new Map<Target, HomeItem>(items.map((item) => [item.target, item])), [items])
  const describeEntry = (entry: DraftWidget) =>
    describe(entry.widget, itemsByTarget.get(entry.widget.target ?? ''), itemsReady)
  // Someone saved while this screen was being edited: saving will be refused.
  const stale = session.baseRevision !== null && revision !== null && session.baseRevision !== revision

  const closeDialog = (open: boolean) => {
    if (!open) setDialog(NO_DIALOG)
  }

  const done = () => {
    void finishEditing(save).then((outcome) => {
      switch (outcome) {
        case 'saved':
          toast.success(t('editor.saved'), { id: 'editor' })
          break
        case 'failed':
          toast.error(t('editor.saveFailed'), { id: 'editor' })
          break
        case 'conflict':
        case 'unchanged':
        case 'busy':
          // The conflict notice is shown by the screen (this one is gone).
          break
      }
    })
  }

  const cancel = () => {
    if (hasUnsavedChanges()) setDialog({ kind: 'discard' })
    else cancelEditing()
  }

  const removeWidget = (pageId: string, entry: DraftWidget, index: number, name: string) => {
    editDraft({ type: 'removeWidget', pageId, key: entry.key })
    toast(t('editor.tile.removed', { name }), {
      id: `removed:${entry.key}`,
      duration: 6_000,
      action: {
        label: t('editor.tile.undo'),
        onClick: () => {
          editDraft({ type: 'restoreWidget', pageId, index, widget: entry })
        },
      },
    })
  }

  let content: ReactNode
  if (page === undefined) {
    content = (
      <div className="flex h-full flex-col items-center justify-center gap-6">
        <EmptyState icon="pages" title={t('editor.noPagesTitle')} body={t('editor.noPagesBody')} />
        <Button
          className="h-auto min-h-14 gap-2 px-6 text-lg [&_svg]:size-6"
          disabled={saving}
          onClick={() => {
            setDialog({ kind: 'addPage' })
          }}
        >
          <Icon name="add" />
          {t('editor.pageList.add')}
        </Button>
      </div>
    )
  } else {
    const count = page.widgets.length
    content = (
      <DashboardGrid columns={page.grid_columns}>
        {page.widgets.map((entry, index) => {
          const description = describeEntry(entry)
          return (
            <DashboardGridItem key={entry.key} colSpan={entry.widget.col_span} rowSpan={entry.widget.row_span}>
              <EditTile
                pageId={page.id}
                columns={page.grid_columns}
                entry={entry}
                index={index}
                count={count}
                description={description}
                highlighted={session.highlightKey === entry.key}
                disabled={saving}
                onRename={() => {
                  setDialog({ kind: 'renameWidget', pageId: page.id, entry })
                }}
                onRemove={() => {
                  removeWidget(page.id, entry, index, description.name)
                }}
              />
            </DashboardGridItem>
          )
        })}
        <DashboardGridItem colSpan={1} rowSpan={1}>
          <Button
            variant="outline"
            className="h-full min-h-36 w-full flex-col gap-2 rounded-xl border-2 border-dashed text-lg [&_svg]:size-8"
            disabled={saving}
            onClick={() => {
              setAddOpen(true)
            }}
          >
            <Icon name="add" />
            {t('editor.add')}
          </Button>
        </DashboardGridItem>
      </DashboardGrid>
    )
  }

  const editBanner = (
    <div role="status" className="px-4 pb-3 md:px-6">
      <div
        className={
          stale
            ? 'flex min-h-14 items-center gap-3 rounded-xl border border-destructive/50 bg-card px-4 py-3 text-base'
            : 'flex min-h-14 items-center gap-3 rounded-xl border border-dashed bg-card px-4 py-3 text-base text-muted-foreground'
        }
      >
        <Icon name={stale ? 'warning' : 'edit'} className={stale ? 'size-6 shrink-0 text-destructive' : 'size-6 shrink-0'} />
        <span>{stale ? t('editor.staleHint') : t('editor.hint')}</span>
      </div>
    </div>
  )

  const renamed = dialog.kind === 'renameWidget' ? dialog : null
  const pageDialog = dialog.kind === 'addPage' || dialog.kind === 'renamePage' ? dialog : null
  const removedPage = dialog.kind === 'removePage' ? dialog.page : null

  return (
    <>
      <AppShell
        pages={shellPages}
        activePageId={page?.id ?? null}
        onSelectPage={onSelectPage}
        banner={
          <>
            {banner}
            {editBanner}
          </>
        }
        headerExtra={
          <>
            <HeaderButton
              icon="add"
              label={t('editor.add')}
              compact
              disabled={saving || page === undefined}
              onClick={() => {
                setAddOpen(true)
              }}
            />
            <HeaderButton
              icon="pages"
              label={t('editor.pages')}
              compact
              disabled={saving}
              onClick={() => {
                setPagesOpen(true)
              }}
            />
            <HeaderButton icon="close" label={t('common.cancel')} variant="outline" disabled={saving} onClick={cancel} />
            <HeaderButton
              icon="check"
              label={saving ? t('editor.saving') : t('editor.done')}
              variant="default"
              busy={saving}
              onClick={done}
            />
          </>
        }
      >
        {content}
      </AppShell>

      {page !== undefined && (
        <AddSheet
          open={addOpen}
          onOpenChange={setAddOpen}
          pageTitle={page.title}
          columns={page.grid_columns}
          items={items}
          itemsReady={itemsReady}
          onPage={new Set(page.widgets.flatMap(({ widget }) => (widget.target === undefined ? [] : [widget.target])))}
          onAdd={(widget) => {
            editDraft({ type: 'addWidget', pageId: page.id, widget })
            setAddOpen(false)
          }}
        />
      )}

      <PagesDialog
        open={pagesOpen}
        onOpenChange={setPagesOpen}
        pages={pages}
        activePageId={page?.id ?? null}
        disabled={saving}
        onShowPage={(pageId) => {
          onSelectPage(pageId)
          setPagesOpen(false)
        }}
        onAddPage={() => {
          setDialog({ kind: 'addPage' })
        }}
        onRenamePage={(target) => {
          setDialog({ kind: 'renamePage', page: target })
        }}
        onRemovePage={(target) => {
          setDialog({ kind: 'removePage', page: target })
        }}
      />

      <PageNameDialog
        open={pageDialog !== null}
        onOpenChange={closeDialog}
        mode={pageDialog?.kind === 'renamePage' ? 'rename' : 'add'}
        title={pageDialog?.kind === 'renamePage' ? pageDialog.page.title : ''}
        icon={pageDialog?.kind === 'renamePage' ? pageDialog.page.icon : undefined}
        onSubmit={(title, icon) => {
          if (pageDialog?.kind === 'renamePage') {
            editDraft({ type: 'editPage', pageId: pageDialog.page.id, title, icon })
            return
          }
          const current = useEditorStore.getState().session
          if (current === null) return
          const id = newPageId(current.draft)
          editDraft({ type: 'addPage', id, title, icon })
          onSelectPage(id)
          setPagesOpen(false)
        }}
      />

      <TileNameDialog
        open={renamed !== null}
        onOpenChange={closeDialog}
        label={renamed === null ? '' : (widgetLabel(renamed.entry.widget) ?? '')}
        ownName={renamed === null ? undefined : describeEntry(renamed.entry).ownName}
        onRename={(label) => {
          if (renamed !== null) editDraft({ type: 'renameWidget', pageId: renamed.pageId, key: renamed.entry.key, label })
        }}
      />

      <ConfirmDialog
        open={removedPage !== null}
        onOpenChange={closeDialog}
        icon="remove"
        title={t('editor.pageList.removeTitle', { name: removedPage?.title ?? '' })}
        description={t('editor.pageList.removeBody')}
        confirmLabel={t('editor.pageList.removeConfirm')}
        confirmVariant="destructive"
        onConfirm={() => {
          if (removedPage !== null) editDraft({ type: 'removePage', pageId: removedPage.id })
        }}
      />

      <ConfirmDialog
        open={dialog.kind === 'discard'}
        onOpenChange={closeDialog}
        title={t('editor.discardTitle')}
        description={t('editor.discardBody')}
        confirmLabel={t('editor.discard')}
        cancelLabel={t('editor.keepEditing')}
        confirmVariant="destructive"
        onConfirm={cancelEditing}
      />
    </>
  )
}
