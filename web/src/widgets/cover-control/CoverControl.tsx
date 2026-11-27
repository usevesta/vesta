import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import type { IconName } from '@/lib/icons'
import { LabeledSlider } from '@/widgets/parts/LabeledSlider'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export type CoverMotion = 'opening' | 'closing' | 'stopped'

export interface CoverControlProps extends WidgetStatusProps {
  /** e.g. "Bedroom blind", "Garage door". */
  label: string
  /** Semantic icon, e.g. "blind", "garage_door", "gate". Default "blind". */
  icon?: IconName
  /** 0 = closed … 100 = fully open. Omit when the device cannot report it. */
  position?: number
  /** Open/closed end-stop, for covers without a position (if known). */
  isOpen?: boolean
  motion?: CoverMotion
  /** Show the Stop button. Default true. */
  canStop?: boolean
  onOpen: () => void
  onClose: () => void
  onStop?: () => void
  /** Present when the cover can move to a given position. */
  onPositionChange?: (position: number) => void
  /** Garage doors and gates: ask before opening (or moving to a position). */
  sensitive?: boolean
}

type PendingConfirmation = { kind: 'open' } | { kind: 'position'; position: number }

export function CoverControl({
  label,
  icon = 'blind',
  position,
  isOpen,
  motion = 'stopped',
  canStop = true,
  onOpen,
  onClose,
  onStop,
  onPositionChange,
  sensitive = false,
  pending,
  unavailable = false,
}: CoverControlProps) {
  const { t } = useTranslation()
  const [confirmation, setConfirmation] = useState<PendingConfirmation | null>(null)

  const subtitle =
    motion === 'opening'
      ? t('cover.opening')
      : motion === 'closing'
        ? t('cover.closing')
        : position !== undefined
          ? t('cover.openPercent', { value: Math.round(position) })
          : isOpen === undefined
            ? undefined
            : isOpen
              ? t('cover.isOpen')
              : t('cover.isClosed')

  const requestOpen = () => {
    if (sensitive) setConfirmation({ kind: 'open' })
    else onOpen()
  }

  const requestPosition = (next: number) => {
    if (onPositionChange === undefined) return
    if (sensitive) setConfirmation({ kind: 'position', position: next })
    else onPositionChange(next)
  }

  const confirm = () => {
    if (confirmation?.kind === 'open') onOpen()
    else if (confirmation?.kind === 'position') onPositionChange?.(confirmation.position)
    setConfirmation(null)
  }

  const actionClass = 'h-auto min-h-16 flex-1 flex-col gap-1 text-base'

  return (
    <WidgetFrame
      icon={icon}
      title={label}
      subtitle={subtitle}
      active={motion !== 'stopped' || (position ?? 0) > 0 || isOpen === true}
      pending={pending}
      unavailable={unavailable}
    >
      {position !== undefined && onPositionChange !== undefined && (
        <LabeledSlider
          label={t('cover.position')}
          value={position}
          min={0}
          max={100}
          disabled={unavailable}
          formatValue={(value) => t('common.percent', { value: Math.round(value) })}
          onCommit={requestPosition}
        />
      )}
      <div className="flex gap-2">
        <Button variant="secondary" className={actionClass} disabled={unavailable} onClick={requestOpen}>
          <Icon name="cover_open" className="size-6" />
          {t('cover.open')}
        </Button>
        {canStop && onStop !== undefined && (
          <Button variant="secondary" className={actionClass} disabled={unavailable} onClick={onStop}>
            <Icon name="cover_stop" className="size-5" />
            {t('cover.stop')}
          </Button>
        )}
        <Button variant="secondary" className={actionClass} disabled={unavailable} onClick={onClose}>
          <Icon name="cover_close" className="size-6" />
          {t('cover.close')}
        </Button>
      </div>
      <ConfirmDialog
        open={confirmation !== null}
        onOpenChange={(open) => {
          if (!open) setConfirmation(null)
        }}
        icon={icon}
        title={t('cover.confirmOpenTitle', { name: label })}
        description={t('cover.confirmOpenBody')}
        confirmLabel={t('cover.open')}
        onConfirm={confirm}
      />
    </WidgetFrame>
  )
}
