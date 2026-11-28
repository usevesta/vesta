import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import type { DoorbellOverlay } from '@/api/generated'
import { CommandButton } from '@/components/CommandButton'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useEndpointView } from '@/live/device-store'
import { dismissDoorbell, useKioskStore } from '@/live/kiosk/kiosk'
import { liveClient } from '@/live/live'
import { openCommand } from '@/live/sensitive'
import { useNow } from '@/lib/use-now'

function AutoCloseCountdown({ until }: { until: number }) {
  const { t } = useTranslation()
  const now = useNow('second')
  const seconds = Math.max(0, Math.ceil((until - now.getTime()) / 1_000))
  return <>{t('doorbell.autoClose', { seconds })}</>
}

export interface DoorbellOverlayViewProps {
  overlay: DoorbellOverlay
}

/**
 * Full-screen "someone is at the door": camera (placeholder until live
 * video lands) and the open-door action, which asks for confirmation and
 * then sends the command as confirmed.
 */
export function DoorbellOverlayView({ overlay }: DoorbellOverlayViewProps) {
  const { t } = useTranslation()
  const ringing = useKioskStore((s) => s.ringing)
  const ringingUntil = useKioskStore((s) => s.ringingUntil)
  const [confirming, setConfirming] = useState(false)
  const doorbell = useEndpointView(overlay.doorbell_target)
  const door = useEndpointView(overlay.open_target)
  const command = door === undefined ? null : openCommand(door.endpoint)
  const doorName = door?.endpoint.name ?? t('widget.unknownDevice')

  // The confirmation stays open (busy) until the door answered.
  const openDoor = () => {
    if (door === undefined || command === null) return undefined
    return liveClient()
      .sendCommand(door.target, command, { label: doorName, confirmed: true })
      .then((outcome) => {
        if (!outcome.ok) return
        toast.success(t('door.opened', { name: doorName }))
        dismissDoorbell()
      })
  }

  return (
    <Dialog
      open={ringing}
      onOpenChange={(open) => {
        if (!open) dismissDoorbell()
      }}
    >
      <DialogContent
        showCloseButton={false}
        className="top-0 left-0 flex h-dvh w-full max-w-none translate-x-0 translate-y-0 flex-col gap-6 rounded-none bg-background p-6 ring-0 sm:max-w-none md:p-10"
      >
        <DialogHeader className="flex-row items-center gap-4 text-left">
          <span className="flex size-16 shrink-0 items-center justify-center rounded-2xl bg-primary text-primary-foreground">
            <Icon name="doorbell" className="size-9 motion-safe:animate-pulse" />
          </span>
          <span className="flex min-w-0 flex-col gap-1">
            <DialogTitle className="text-3xl leading-tight">{t('doorbell.title')}</DialogTitle>
            <DialogDescription className="text-lg">{doorbell?.endpoint.name ?? ''}</DialogDescription>
          </span>
        </DialogHeader>

        <div className="flex min-h-40 flex-1 flex-col items-center justify-center gap-4 rounded-2xl bg-muted p-6 text-center text-muted-foreground">
          <Icon name="video_off" className="size-16" />
          <p className="text-xl">{t('camera.notAvailable')}</p>
        </div>

        <div className="flex flex-col gap-3 sm:flex-row">
          <Button variant="secondary" className="h-auto min-h-16 flex-1 gap-3 text-xl" onClick={dismissDoorbell}>
            <Icon name="close" className="size-6" />
            {t('doorbell.dismiss')}
          </Button>
          {command !== null && (
            <CommandButton
              className="h-auto min-h-16 flex-1 gap-3 text-xl"
              disabled={door?.available !== true}
              busy={door?.pending === true}
              icon="door_open"
              iconClassName="size-6"
              onClick={() => {
                setConfirming(true)
              }}
            >
              {door?.pending === true ? t('door.opening') : t('doorbell.open')}
            </CommandButton>
          )}
        </div>
        {ringingUntil !== null && (
          <p className="text-center text-sm text-muted-foreground" aria-live="off">
            <AutoCloseCountdown until={ringingUntil} />
          </p>
        )}

        <ConfirmDialog
          open={confirming}
          onOpenChange={setConfirming}
          icon="door"
          title={t('door.confirmTitle', { name: doorName })}
          description={t('door.confirmBody')}
          confirmLabel={t('door.confirm')}
          onConfirm={openDoor}
        />
      </DialogContent>
    </Dialog>
  )
}
