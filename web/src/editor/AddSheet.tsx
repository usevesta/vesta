import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { Target, Widget } from '@/api/generated'
import { Icon } from '@/components/Icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { extraWidget, groupByRoom, widgetForItem, type HomeItem } from '@/editor/model'
import { isIconName, type IconName } from '@/lib/icons'
import { EXTRA_WIDGET_TYPES, widgetMeta, type WidgetType } from '@/widgets/registry'

export interface AddSheetProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  pageTitle: string
  columns: number
  items: readonly HomeItem[]
  /** The home's list of things has arrived (else: "waiting"). */
  itemsReady: boolean
  /** Targets already shown on this page. */
  onPage: ReadonlySet<Target>
  /** Add `widget`; `name` is what the person tapped. */
  onAdd: (widget: Widget, name: string) => void
}

function ItemButton({
  icon,
  name,
  detail,
  badge,
  onClick,
}: {
  icon: IconName
  name: string
  detail?: string
  badge?: ReactNode
  onClick: () => void
}) {
  return (
    <li>
      <Button
        variant="ghost"
        className="h-auto min-h-16 w-full justify-start gap-4 rounded-xl px-3 py-2 text-left whitespace-normal"
        onClick={onClick}
      >
        <span className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground">
          <Icon name={icon} className="size-6" />
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-lg font-medium">{name}</span>
          {detail !== undefined && <span className="truncate text-sm text-muted-foreground">{detail}</span>}
        </span>
        {badge}
        <Icon name="add" className="size-6 text-muted-foreground" />
      </Button>
    </li>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      <h3 className="px-3 pt-4 pb-1 text-sm font-medium text-muted-foreground">{title}</h3>
      <ul className="flex flex-col gap-1">{children}</ul>
    </section>
  )
}

/**
 * Everything in the home that a tile can show, by room and with plain
 * names. Tapping one adds the right tile (picked by what it is).
 */
export function AddSheet({ open, onOpenChange, pageTitle, columns, items, itemsReady, onPage, onAdd }: AddSheetProps) {
  const { t } = useTranslation()
  const groups = groupByRoom(items)

  // Names of tiles that show no device. One without a name here is not offered.
  const extraNames: Partial<Record<WidgetType, string>> = { 'clock-weather': t('editor.clock') }
  const extras = EXTRA_WIDGET_TYPES.flatMap((type) => {
    const name = extraNames[type]
    return name === undefined ? [] : [{ type, name }]
  })

  let things: ReactNode
  if (groups.length === 0) {
    things = (
      <div className="flex flex-col items-center gap-3 px-6 py-10 text-center">
        <span className="flex size-16 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
          <Icon
            name={itemsReady ? 'home' : 'loading'}
            className={itemsReady ? 'size-8' : 'size-8 motion-safe:animate-spin'}
          />
        </span>
        <p className="text-lg font-medium">{itemsReady ? t('editor.addSheet.emptyTitle') : t('editor.addSheet.waiting')}</p>
        {itemsReady && <p className="text-base text-muted-foreground">{t('editor.addSheet.emptyBody')}</p>}
      </div>
    )
  } else {
    things = groups.map((group) => (
      <Section key={group.room?.id ?? ''} title={group.room?.name ?? t('editor.addSheet.noRoom')}>
        {group.items.map((item) => (
          <ItemButton
            key={item.target}
            icon={isIconName(item.type) ? item.type : 'unknown'}
            name={item.name}
            detail={item.partOf}
            badge={
              onPage.has(item.target) ? (
                <Badge variant="secondary" className="h-auto shrink-0 px-2 py-1 text-sm">
                  {t('editor.addSheet.onPage')}
                </Badge>
              ) : undefined
            }
            onClick={() => {
              const widget = widgetForItem(item, columns)
              if (widget !== null) onAdd(widget, item.name)
            }}
          />
        ))}
      </Section>
    ))
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" showCloseButton={false} className="w-full gap-0 p-0 sm:max-w-md">
        <SheetHeader className="flex-row items-start gap-3 border-b p-4 pl-6">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <SheetTitle className="truncate text-2xl">{t('editor.addSheet.title', { page: pageTitle })}</SheetTitle>
            <SheetDescription className="text-base">{t('editor.addSheet.description')}</SheetDescription>
          </div>
          <SheetClose asChild>
            <Button variant="ghost" className="size-14 rounded-xl" aria-label={t('common.close')}>
              <Icon name="close" className="size-7" />
            </Button>
          </SheetClose>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-[max(1.5rem,env(safe-area-inset-bottom))]">
          {things}
          <Section title={t('editor.addSheet.more')}>
            {extras.map(({ type, name }) => (
              <ItemButton
                key={type}
                icon={widgetMeta[type].icon}
                name={name}
                onClick={() => {
                  const widget = extraWidget(type, columns)
                  if (widget !== null) onAdd(widget, name)
                }}
              />
            ))}
          </Section>
        </div>
      </SheetContent>
    </Sheet>
  )
}
