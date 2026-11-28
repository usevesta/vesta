import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import type { IconName } from '@/lib/icons'
import { cn } from '@/lib/utils'

export interface BusyIconProps {
  /** Show the busy indicator instead of `name`. */
  busy: boolean
  /** The control's own icon, shown while it is not busy. */
  name?: IconName
  className?: string
}

/**
 * A control's icon, or "working on it" in its place: a spinner, or a
 * still hourglass when the screen asks for less motion. Screen readers
 * hear "Working…".
 */
export function BusyIcon({ busy, name, className }: BusyIconProps) {
  const { t } = useTranslation()
  if (!busy) return name === undefined ? null : <Icon name={name} className={className} />
  return (
    <span data-slot="busy-icon" className="inline-flex shrink-0 items-center justify-center">
      <Icon name="loading" className={cn(className, 'animate-spin motion-reduce:hidden')} />
      <Icon name="busy" className={cn(className, 'hidden motion-reduce:inline')} />
      <span className="sr-only">{t('widget.pending')}</span>
    </span>
  )
}
