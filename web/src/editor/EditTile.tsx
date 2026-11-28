import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { editDraft } from '@/editor/editor-store'
import type { TileDescription } from '@/editor/describe'
import type { DraftWidget } from '@/editor/model'
import { cn } from '@/lib/utils'
import { nextSize, sizeOf } from '@/widgets/sizes'

export interface EditTileProps {
  pageId: string
  columns: number
  entry: DraftWidget
  index: number
  count: number
  description: TileDescription
  highlighted: boolean
  disabled: boolean
  onRename: () => void
  onRemove: () => void
}

/**
 * A tile in edit mode: what it shows, plus big buttons to move it, change
 * its size, rename and remove it. Live controls are not shown, so nothing
 * in the home is switched by accident while arranging.
 */
export function EditTile({
  pageId,
  columns,
  entry,
  index,
  count,
  description,
  highlighted,
  disabled,
  onRename,
  onRemove,
}: EditTileProps) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const { key, widget } = entry
  const span = { colSpan: widget.col_span, rowSpan: widget.row_span }
  const size = sizeOf(span, columns)
  const sizeLabel = size === null ? t('editor.size.custom') : t(`editor.size.${size}`)
  const { name } = description

  useEffect(() => {
    // Keep the tile in view after it moved, grew or was added.
    if (highlighted) ref.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }, [highlighted, index, span.colSpan, span.rowSpan])

  const move = (by: -1 | 1) => {
    editDraft({ type: 'moveWidget', pageId, key, by })
  }
  const iconButton = 'size-14 rounded-xl'
  const textButton = 'h-14 min-w-14 gap-2 rounded-xl px-3 text-base'

  return (
    <Card
      ref={ref}
      data-slot="edit-tile"
      data-highlighted={highlighted || undefined}
      className={cn(
        '@container h-full gap-3 border-2 border-dashed border-border text-base ring-0 transition-shadow',
        highlighted && 'border-solid border-primary ring-2 ring-primary/40',
      )}
    >
      <div className="flex min-h-12 items-center gap-3 px-4">
        <span
          className={cn(
            'flex size-12 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground',
            description.icon === 'warning' && 'text-destructive',
          )}
        >
          <Icon name={description.icon} className="size-6" />
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="line-clamp-2 text-lg leading-tight font-medium break-words">{name}</span>
          {description.detail !== undefined && (
            <span className="truncate text-sm text-muted-foreground">{description.detail}</span>
          )}
        </span>
      </div>
      <div className="mt-auto flex flex-wrap gap-2 px-4">
        <Button
          variant="secondary"
          className={iconButton}
          disabled={disabled || index === 0}
          aria-label={t('editor.tile.moveEarlier', { name })}
          onClick={() => {
            move(-1)
          }}
        >
          <Icon name="move_earlier" className="size-6" />
        </Button>
        <Button
          variant="secondary"
          className={iconButton}
          disabled={disabled || index === count - 1}
          aria-label={t('editor.tile.moveLater', { name })}
          onClick={() => {
            move(1)
          }}
        >
          <Icon name="move_later" className="size-6" />
        </Button>
        <Button
          variant="secondary"
          className={textButton}
          disabled={disabled}
          aria-label={t('editor.tile.resize', { name, size: sizeLabel })}
          onClick={() => {
            editDraft({ type: 'resizeWidget', pageId, key, size: nextSize(span, columns) })
          }}
        >
          <Icon name="resize" className="size-6" />
          <span className="hidden @[22rem]:inline">{sizeLabel}</span>
        </Button>
        <Button
          variant="secondary"
          className={textButton}
          disabled={disabled}
          aria-label={t('editor.tile.renameLabel', { name })}
          onClick={onRename}
        >
          <Icon name="rename" className="size-6" />
          <span className="hidden @[30rem]:inline">{t('editor.tile.rename')}</span>
        </Button>
        <Button
          variant="destructive"
          className={textButton}
          disabled={disabled}
          aria-label={t('editor.tile.removeLabel', { name })}
          onClick={onRemove}
        >
          <Icon name="remove" className="size-6" />
          <span className="hidden @[30rem]:inline">{t('editor.tile.remove')}</span>
        </Button>
      </div>
    </Card>
  )
}
