import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Card } from '@/components/ui/card'
import type { IconName } from '@/lib/icons'
import { cn } from '@/lib/utils'
import type { WidgetStatusProps } from '@/widgets/types'

export interface WidgetFrameProps extends WidgetStatusProps {
  icon: IconName
  title: string
  /** Short state line under the title, e.g. "On · 80%". */
  subtitle?: ReactNode
  /** Highlights the icon (light on, fan running, cover moving). */
  active?: boolean
  /** Control at the end of the header row, e.g. an on/off switch. */
  headerAction?: ReactNode
  /**
   * id of the header control. The whole header row then acts as its label,
   * so the touch target is the full width of the tile.
   */
  headerControlId?: string
  className?: string
  children?: ReactNode
}

/** Common tile chrome: icon, name, state line, pending and unavailable states. */
export function WidgetFrame({
  icon,
  title,
  subtitle,
  active = false,
  headerAction,
  headerControlId,
  pending = false,
  unavailable = false,
  className,
  children,
}: WidgetFrameProps) {
  const { t } = useTranslation()

  const status = unavailable ? (
    <span className="inline-flex items-center gap-1.5">
      <Icon name="unavailable" className="size-4" />
      {t('widget.unavailable')}
    </span>
  ) : (
    subtitle
  )

  const header = (
    <>
      <span
        className={cn(
          'flex size-12 shrink-0 items-center justify-center rounded-xl transition-colors',
          active && !unavailable ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground',
        )}
      >
        <Icon name={icon} className="size-6" />
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="line-clamp-2 text-lg leading-tight font-medium break-words">{title}</span>
        {status != null && <span className="truncate text-sm text-muted-foreground">{status}</span>}
      </span>
      {pending && (
        <span className="shrink-0 text-muted-foreground">
          <Icon name="loading" className="size-5 motion-safe:animate-spin" />
          <span className="sr-only">{t('widget.pending')}</span>
        </span>
      )}
      {headerAction}
    </>
  )

  const headerClass = 'flex min-h-14 items-center gap-3 px-(--card-spacing)'

  return (
    <Card
      data-slot="widget"
      data-pending={pending || undefined}
      data-unavailable={unavailable || undefined}
      aria-busy={pending || undefined}
      className={cn(
        'glass-surface h-full gap-4 text-base transition-[opacity,box-shadow] [--card-spacing:--spacing(4)] md:[--card-spacing:--spacing(5)]',
        pending && 'ring-2 ring-primary/50',
        unavailable && 'opacity-60',
        className,
      )}
    >
      {headerControlId === undefined ? (
        <div className={headerClass}>{header}</div>
      ) : (
        <label
          htmlFor={headerControlId}
          className={cn(headerClass, unavailable ? 'cursor-default' : 'cursor-pointer')}
        >
          {header}
        </label>
      )}
      {children != null && (
        <div className="flex flex-1 flex-col justify-end gap-4 px-(--card-spacing)">{children}</div>
      )}
    </Card>
  )
}
