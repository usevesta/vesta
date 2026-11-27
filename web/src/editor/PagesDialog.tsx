import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { editDraft } from '@/editor/editor-store'
import { MAX_PAGES, type DraftPage } from '@/editor/model'
import { cn } from '@/lib/utils'

export interface PagesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  pages: readonly DraftPage[]
  activePageId: string | null
  disabled: boolean
  onShowPage: (pageId: string) => void
  onAddPage: () => void
  onRenamePage: (page: DraftPage) => void
  onRemovePage: (page: DraftPage) => void
}

/** All pages of the screen: reorder, rename, remove, add. */
export function PagesDialog({
  open,
  onOpenChange,
  pages,
  activePageId,
  disabled,
  onShowPage,
  onAddPage,
  onRenamePage,
  onRemovePage,
}: PagesDialogProps) {
  const { t } = useTranslation()
  const full = pages.length >= MAX_PAGES
  const iconButton = 'size-14 rounded-xl'

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col gap-4 p-6 sm:max-w-xl" showCloseButton={false}>
        <DialogHeader className="flex-row items-start gap-3">
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <DialogTitle className="text-2xl">{t('editor.pageList.title')}</DialogTitle>
            <DialogDescription className="text-base">{t('editor.pageList.description')}</DialogDescription>
          </div>
          <Button
            variant="ghost"
            className="-mt-2 -mr-2 size-14 rounded-xl"
            aria-label={t('common.close')}
            onClick={() => {
              onOpenChange(false)
            }}
          >
            <Icon name="close" className="size-7" />
          </Button>
        </DialogHeader>

        <ol className="-mx-2 flex min-h-0 flex-col gap-2 overflow-y-auto px-2">
          {pages.map((page, index) => {
            const name = page.title
            const active = page.id === activePageId
            return (
              <li
                key={page.id}
                className={cn(
                  'flex flex-wrap items-center gap-2 rounded-xl border p-2',
                  active ? 'border-primary bg-accent' : 'border-border',
                )}
              >
                <Button
                  variant="ghost"
                  className="h-auto min-h-14 min-w-0 flex-1 basis-40 justify-start gap-3 rounded-xl px-3 text-left text-lg whitespace-normal"
                  aria-current={active ? 'page' : undefined}
                  onClick={() => {
                    onShowPage(page.id)
                  }}
                >
                  <Icon name={page.icon ?? 'home'} className="size-7 shrink-0" />
                  <span className="min-w-0 truncate font-medium">{name}</span>
                </Button>
                <div className="flex gap-2">
                  <Button
                    variant="secondary"
                    className={iconButton}
                    disabled={disabled || index === 0}
                    aria-label={t('editor.pageList.moveUp', { name })}
                    onClick={() => {
                      editDraft({ type: 'movePage', pageId: page.id, by: -1 })
                    }}
                  >
                    <Icon name="move_up" className="size-6" />
                  </Button>
                  <Button
                    variant="secondary"
                    className={iconButton}
                    disabled={disabled || index === pages.length - 1}
                    aria-label={t('editor.pageList.moveDown', { name })}
                    onClick={() => {
                      editDraft({ type: 'movePage', pageId: page.id, by: 1 })
                    }}
                  >
                    <Icon name="move_down" className="size-6" />
                  </Button>
                  <Button
                    variant="secondary"
                    className={iconButton}
                    disabled={disabled}
                    aria-label={t('editor.pageList.rename', { name })}
                    onClick={() => {
                      onRenamePage(page)
                    }}
                  >
                    <Icon name="rename" className="size-6" />
                  </Button>
                  <Button
                    variant="destructive"
                    className={iconButton}
                    disabled={disabled}
                    aria-label={t('editor.pageList.remove', { name })}
                    onClick={() => {
                      onRemovePage(page)
                    }}
                  >
                    <Icon name="remove" className="size-6" />
                  </Button>
                </div>
              </li>
            )
          })}
        </ol>

        <Button className="h-auto min-h-14 gap-2 text-lg" disabled={disabled || full} onClick={onAddPage}>
          <Icon name="add" className="size-6" />
          {t('editor.pageList.add')}
        </Button>
        {full && <p className="text-center text-sm text-muted-foreground">{t('editor.pageList.full')}</p>}
      </DialogContent>
    </Dialog>
  )
}
