import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { useNow } from '@/lib/use-now'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface ClockWeatherProps extends WidgetStatusProps {
  /** Optional place name shown under the date. */
  label?: string
}

/** Live clock (local time) with a weather placeholder. */
export function ClockWeather({ label, pending, unavailable }: ClockWeatherProps) {
  const { t, i18n } = useTranslation()
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
    <WidgetFrame
      icon="weather"
      title={dateFormat.format(now)}
      subtitle={label}
      pending={pending}
      unavailable={unavailable}
    >
      <time
        dateTime={now.toISOString()}
        className="text-6xl leading-none font-semibold tracking-tight tabular-nums md:text-7xl"
      >
        {timeFormat.format(now)}
      </time>
      <p className="inline-flex items-center gap-2 text-sm text-muted-foreground">
        <Icon name="weather" className="size-4" />
        {t('clock.weatherSoon')}
      </p>
    </WidgetFrame>
  )
}
