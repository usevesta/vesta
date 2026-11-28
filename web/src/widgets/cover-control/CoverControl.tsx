import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CommandButton } from '@/components/CommandButton'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import type { CommandHandler } from '@/lib/command-gate'
import type { IconName } from '@/lib/icons'
import { useCommand } from '@/lib/use-command'
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
  /** Command callbacks: a promise they return keeps the control busy. */
  onOpen: CommandHandler
  onClose: CommandHandler
  onStop?: CommandHandler
  /** Present when the cover can move to a given position. */
  onPositionChange?: CommandHandler<[number]>
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
  // One per button: Stop stays usable while the cover is opening.
  const open = useCommand(onOpen)
  const close = useCommand(onClose)
  const stop = useCommand(onStop)

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
    else open.run()
  }

  const requestPosition = (next: number) => {
    if (onPositionChange === undefined) return undefined
    if (sensitive) {
      setConfirmation({ kind: 'position', position: next })
      return undefined
    }
    return onPositionChange(next)
  }

  // The dialog stays open (busy) until the command is done.
  const confirm = () => {
    if (confirmation?.kind === 'open') return onOpen()
    if (confirmation?.kind === 'position') return onPositionChange?.(confirmation.position)
    return undefined
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
        <CommandButton
          variant="secondary"
          className={actionClass}
          disabled={unavailable}
          busy={open.busy}
          icon="cover_open"
          iconClassName="size-6"
          onClick={requestOpen}
        >
          {t('cover.open')}
        </CommandButton>
        {canStop && onStop !== undefined && (
          <CommandButton
            variant="secondary"
            className={actionClass}
            disabled={unavailable}
            busy={stop.busy}
            icon="cover_stop"
            onClick={() => {
              stop.run()
            }}
          >
            {t('cover.stop')}
          </CommandButton>
        )}
        <CommandButton
          variant="secondary"
          className={actionClass}
          disabled={unavailable}
          busy={close.busy}
          icon="cover_close"
          iconClassName="size-6"
          onClick={() => {
            close.run()
          }}
        >
          {t('cover.close')}
        </CommandButton>
      </div>
      <ConfirmDialog
        open={confirmation !== null}
        onOpenChange={(next) => {
          if (!next) setConfirmation(null)
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
