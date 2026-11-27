import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'

export interface HeaderButtonProps {
  icon: 'add' | 'pages' | 'close' | 'check' | 'edit'
  label: string
  /** Hide the text on phones (the icon stays, the label is still announced). */
  compact?: boolean
  variant?: 'default' | 'secondary' | 'outline' | 'ghost'
  disabled?: boolean
  /** Working: shows a spinner and cannot be tapped again. */
  busy?: boolean
  onClick: () => void
}

/** Big, labelled button in the screen's header (Edit, Add, Pages, Cancel, Done). */
export function HeaderButton({
  icon,
  label,
  compact = false,
  variant = 'secondary',
  disabled = false,
  busy = false,
  onClick,
}: HeaderButtonProps) {
  return (
    <Button
      variant={variant}
      className="h-14 min-w-14 gap-2 rounded-xl px-4 text-base [&_svg]:size-6"
      aria-label={label}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      onClick={onClick}
    >
      <Icon name={busy ? 'loading' : icon} className={busy ? 'motion-safe:animate-spin' : undefined} />
      <span className={compact ? 'hidden sm:inline' : undefined}>{label}</span>
    </Button>
  )
}
