import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
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
   */
  onOpen: () => void
}

export function DoorButton({ label, icon = 'door', onOpen, pending = false, unavailable = false }: DoorButtonProps) {
  const { t } = useTranslation()
  const [confirming, setConfirming] = useState(false)

  return (
    <WidgetFrame icon={icon} title={label} pending={pending} unavailable={unavailable} active={pending}>
      <Button
        size="lg"
        className="h-auto min-h-16 w-full gap-3 text-xl"
        disabled={unavailable || pending}
        onClick={() => {
          setConfirming(true)
        }}
      >
        <Icon name={pending ? 'loading' : 'door_open'} className={pending ? 'size-6 motion-safe:animate-spin' : 'size-6'} />
        {pending ? t('door.opening') : t('door.open')}
      </Button>
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
