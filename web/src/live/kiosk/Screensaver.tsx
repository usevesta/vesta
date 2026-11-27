import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { selectScreensaverVisible, useKioskStore, wakeScreen } from '@/live/kiosk/kiosk'
import { useNow } from '@/lib/use-now'

function ScreensaverClock() {
  const { i18n } = useTranslation()
  const now = useNow('minute')
  const language = i18n.resolvedLanguage ?? i18n.language
  const [timeFormat, dateFormat] = useMemo(
    () => [
      new Intl.DateTimeFormat(language, { hour: '2-digit', minute: '2-digit' }),
      new Intl.DateTimeFormat(language, { weekday: 'long', day: 'numeric', month: 'long' }),
    ],
    [language],
  )
  return (
    <>
      <time dateTime={now.toISOString()} className="text-8xl leading-none font-semibold tracking-tight tabular-nums md:text-9xl">
        {timeFormat.format(now)}
      </time>
      <span className="text-2xl">{dateFormat.format(now)}</span>
    </>
  )
}

/**
 * Dim full-screen overlay while nobody uses the panel. The tap that wakes
 * it is swallowed here, so it never switches anything underneath.
 */
export function Screensaver() {
  const { t } = useTranslation()
  const visible = useKioskStore(selectScreensaverVisible)
  if (!visible) return null
  return (
    <button
      type="button"
      aria-label={t('screensaver.hint')}
      className="fixed inset-0 z-60 flex cursor-default flex-col items-center justify-center gap-4 bg-scrim text-scrim-foreground outline-none select-none"
      onClick={wakeScreen}
    >
      <ScreensaverClock />
      <span className="mt-8 text-base">{t('screensaver.hint')}</span>
    </button>
  )
}
