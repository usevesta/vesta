import { useTranslation } from 'react-i18next'

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
import type { IconName } from '@/lib/icons'

export interface ConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  confirmLabel: string
  icon?: IconName
  /** Runs only when the person confirms. Cancel is focused by default. */
  onConfirm: () => void
}

/** Large, touch-friendly confirmation for sensitive actions (doors, gates). */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  icon,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation()
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent className="gap-6 p-6 data-[size=default]:max-w-md data-[size=default]:sm:max-w-md">
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
          <AlertDialogCancel className="h-auto min-h-14 flex-1 text-lg">{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction className="h-auto min-h-14 flex-1 text-lg" onClick={onConfirm}>
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
