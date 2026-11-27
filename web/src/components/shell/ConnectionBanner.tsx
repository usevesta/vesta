import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { useConnectionStore } from '@/lib/ws/connection-store'
import { useNow } from '@/lib/use-now'

export interface ConnectionBannerProps {
  /** Skip the wait and try again now. */
  onRetry: () => void
}

function RetryCountdown({ at }: { at: number }) {
  const { t } = useTranslation()
  const now = useNow('second')
  const seconds = Math.max(0, Math.ceil((at - now.getTime()) / 1_000))
  return <>{seconds > 0 ? t('connection.retryingIn', { seconds }) : t('connection.retrying')}</>
}

/** Plain-language connection state. Hidden while connected. */
export function ConnectionBanner({ onRetry }: ConnectionBannerProps) {
  const { t } = useTranslation()
  const status = useConnectionStore((s) => s.status)
  const nextRetryAt = useConnectionStore((s) => s.nextRetryAt)
  const lastOpenedAt = useConnectionStore((s) => s.lastOpenedAt)

  if (status === 'open' || status === 'idle' || status === 'stopped') return null

  const firstConnect = status === 'connecting' && lastOpenedAt === null
  return (
    <div role="status" aria-live="polite" className="px-4 pb-3 md:px-6">
      <div className="glass-surface flex min-h-16 flex-wrap items-center gap-3 rounded-xl border bg-card px-4 py-3">
        <Icon
          name={firstConnect ? 'loading' : 'connection_lost'}
          className={firstConnect ? 'size-6 text-muted-foreground motion-safe:animate-spin' : 'size-6 text-destructive'}
        />
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="text-base font-medium">
            {firstConnect ? t('connection.connecting') : t('connection.lost')}
          </span>
          {!firstConnect && (
            <span className="text-sm text-muted-foreground">
              {status === 'waiting' && nextRetryAt !== null ? (
                <RetryCountdown at={nextRetryAt} />
              ) : (
                t('connection.retrying')
              )}
            </span>
          )}
        </div>
        {status === 'waiting' && (
          <Button variant="secondary" className="h-auto min-h-14 gap-2 px-4 text-base" onClick={onRetry}>
            <Icon name="retry" className="size-5" />
            {t('connection.retryNow')}
          </Button>
        )}
      </div>
    </div>
  )
}
