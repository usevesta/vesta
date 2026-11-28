import type { ComponentProps } from 'react'

import { BusyIcon } from '@/components/BusyIcon'
import { Button } from '@/components/ui/button'
import type { IconName } from '@/lib/icons'
import { cn } from '@/lib/utils'

export interface CommandButtonProps extends ComponentProps<typeof Button> {
  /** Its command is in flight: disabled, marked busy, spinner shown. */
  busy?: boolean
  /** Shown before the label; the spinner takes its place while busy. */
  icon?: IconName
  iconClassName?: string
}

/**
 * A button that sends a command (pause, start, open, Save…). While the
 * command is in flight it cannot be pressed again: it shows a spinner in
 * place of its icon (or in its corner when it has none) and `aria-busy`.
 * Use with `useCommand`.
 */
export function CommandButton({
  busy = false,
  icon,
  iconClassName = 'size-5',
  disabled,
  className,
  children,
  ...props
}: CommandButtonProps) {
  return (
    <Button
      {...props}
      disabled={disabled === true || busy}
      aria-busy={busy || undefined}
      data-busy={busy || undefined}
      // Busy is not unavailable: keep it readable.
      className={cn('relative aria-busy:cursor-progress aria-busy:opacity-100', className)}
    >
      {icon === undefined ? (
        busy && (
          <span className="absolute top-1.5 right-1.5">
            <BusyIcon busy className="size-4" />
          </span>
        )
      ) : (
        <BusyIcon busy={busy} name={icon} className={iconClassName} />
      )}
      {children}
    </Button>
  )
}
