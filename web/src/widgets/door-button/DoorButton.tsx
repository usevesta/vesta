import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CommandButton } from '@/components/CommandButton'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import type { CommandHandler } from '@/lib/command-gate'
import type { IconName } from '@/lib/icons'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface DoorButtonProps extends WidgetStatusProps {
  /** e.g. "Front door". */
  label: string
  /** Semantic icon. Default "door". */
  icon?: IconName
  /**
   * Opening is sensitive: this runs only after the person confirms in a
   * dialog. Biometric checks on phones happen upstream, before sending.
   * A promise it returns keeps the dialog, then the button, busy.
   */
  onOpen: CommandHandler
}

export function DoorButton({ label, icon = 'door', onOpen, pending = false, unavailable = false }: DoorButtonProps) {
  const { t } = useTranslation()
  const [confirming, setConfirming] = useState(false)

  return (
    <WidgetFrame icon={icon} title={label} pending={pending} unavailable={unavailable} active={pending}>
      <CommandButton
        size="lg"
        className="h-auto min-h-16 w-full gap-3 text-xl"
        disabled={unavailable}
        busy={pending}
        icon="door_open"
        iconClassName="size-6"
        onClick={() => {
          setConfirming(true)
        }}
      >
        {pending ? t('door.opening') : t('door.open')}
      </CommandButton>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        icon={icon}
        title={t('door.confirmTitle', { name: label })}
        description={t('door.confirmBody')}
        confirmLabel={t('door.confirm')}
        onConfirm={onOpen}
      />
    </WidgetFrame>
  )
}
