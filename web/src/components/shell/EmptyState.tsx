import { Icon } from '@/components/Icon'
import type { IconName } from '@/lib/icons'

export interface EmptyStateProps {
  icon: IconName
  title: string
  body: string
}

export function EmptyState({ icon, title, body }: EmptyStateProps) {
  return (
    <div className="flex h-full min-h-64 flex-col items-center justify-center gap-4 text-center">
      <span className="flex size-20 items-center justify-center rounded-3xl bg-muted text-muted-foreground">
        <Icon name={icon} className="size-10" />
      </span>
      <h2 className="text-2xl font-semibold">{title}</h2>
      <p className="max-w-md text-lg text-muted-foreground">{body}</p>
    </div>
  )
}
