import { useTranslation } from 'react-i18next'

import { BusyIcon } from '@/components/BusyIcon'
import { Icon } from '@/components/Icon'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { isPromiseLike, type CommandHandler } from '@/lib/command-gate'
import type { IconName } from '@/lib/icons'
import { useCommand } from '@/lib/use-command'

export interface ConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  confirmLabel: string
  /** Default "Cancel". */
  cancelLabel?: string
  /** `destructive` for removing things. */
  confirmVariant?: 'default' | 'destructive'
  icon?: IconName
  /**
   * Runs only when the person confirms. Cancel is focused by default. When
   * it returns a promise (a command on its way), the dialog stays open with
   * its button busy, and closes once it settled.
   */
  onConfirm: CommandHandler
}

/** Large, touch-friendly confirmation for sensitive actions (doors, gates). */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  cancelLabel,
  confirmVariant = 'default',
  icon,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation()
  const confirm = useCommand(() => {
    const result = onConfirm()
    if (isPromiseLike(result)) {
      return Promise.resolve(result).finally(() => {
        onOpenChange(false)
      })
    }
    onOpenChange(false)
    return undefined
  })
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        // Not while the command is on its way.
        if (next || !confirm.busy) onOpenChange(next)
      }}
    >
      <AlertDialogContent
        aria-busy={confirm.busy || undefined}
        className="gap-6 p-6 data-[size=default]:max-w-md data-[size=default]:sm:max-w-md"
      >
        <AlertDialogHeader>
          {icon !== undefined && (
            <AlertDialogMedia className="size-14 rounded-xl">
              <Icon name={icon} className="size-8" />
            </AlertDialogMedia>
          )}
          <AlertDialogTitle className="text-2xl">{title}</AlertDialogTitle>
          <AlertDialogDescription className="text-base">{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter className="-mx-6 -mb-6 p-6">
          <AlertDialogCancel className="h-auto min-h-14 flex-1 text-lg" disabled={confirm.busy}>
            {cancelLabel ?? t('common.cancel')}
          </AlertDialogCancel>
          <AlertDialogAction
            variant={confirmVariant}
            className="h-auto min-h-14 flex-1 gap-2 text-lg aria-busy:cursor-progress aria-busy:opacity-100"
            disabled={confirm.busy}
            aria-busy={confirm.busy || undefined}
            onClick={(event) => {
              // It closes itself once the command is done.
              event.preventDefault()
              confirm.run()
            }}
          >
            <BusyIcon busy={confirm.busy} className="size-5" />
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
