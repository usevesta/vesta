import { useId } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Switch } from '@/components/ui/switch'
import type { IconName } from '@/lib/icons'
import { cn } from '@/lib/utils'
import {
  currentReading,
  energyReading,
  IDLE_POWER_W,
  powerReading,
  readingText,
  voltageReading,
} from '@/widgets/outlet-control/readings'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface OutletControlProps extends WidgetStatusProps {
  /** e.g. "Hall lamp". */
  label: string
  /** Semantic icon. Default "outlet". */
  icon?: IconName
  /** Unknown until the plug reported. */
  on?: boolean
  /**
   * Present when the plug can be switched from here. Absent (a plug that
   * is only watched): the tile shows whether it is on, without a switch.
   */
  onToggle?: (on: boolean) => void
  /** What it draws right now, W. */
  powerW?: number
  /** Lifetime energy, Wh (only plugs that count it). */
  energyWh?: number
  voltageV?: number
  currentA?: number
}

/**
 * A plug or switch: on/off, and what it draws when it measures. Large
 * touch target for the switch; a plug that is only watched shows its
 * state in words instead.
 */
export function OutletControl({
  label,
  icon = 'outlet',
  on,
  onToggle,
  powerW,
  energyWh,
  voltageV,
  currentA,
  pending,
  unavailable = false,
}: OutletControlProps) {
  const { t, i18n } = useTranslation()
  const switchId = useId()
  const language = i18n.language
  const isOn = on === true
  const drawing = isOn && powerW !== undefined && powerW >= IDLE_POWER_W
  const power = powerW === undefined ? null : powerReading(powerW, language)

  const state = on === undefined ? undefined : isOn ? t('common.on') : t('common.off')
  const subtitle =
    state === undefined ? undefined : drawing && power !== null ? `${state} · ${readingText(power)}` : state

  const details: string[] = []
  if (voltageV !== undefined) details.push(readingText(voltageReading(voltageV, language)))
  if (currentA !== undefined) details.push(readingText(currentReading(currentA, language)))

  const control =
    onToggle === undefined ? undefined : (
      <Switch
        id={switchId}
        size="lg"
        checked={isOn}
        disabled={unavailable || on === undefined}
        aria-label={isOn ? t('widget.turnOff', { name: label }) : t('widget.turnOn', { name: label })}
        onCheckedChange={onToggle}
      />
    )

  return (
    <div className="h-full min-w-0" data-slot="outlet" data-on={on === undefined ? undefined : String(isOn)}>
      <WidgetFrame
        icon={icon}
        title={label}
        subtitle={
          subtitle === undefined ? undefined : (
            <span role="status" aria-label={isOn ? t('outlet.isOn', { name: label }) : t('outlet.isOff', { name: label })}>
              {subtitle}
            </span>
          )
        }
        active={isOn}
        pending={pending}
        unavailable={unavailable}
        headerControlId={onToggle === undefined ? undefined : switchId}
        headerAction={control}
      >
        {power !== null && (
          <div className="flex flex-wrap items-end justify-between gap-x-4 gap-y-1">
            <span className="flex items-baseline gap-1.5" aria-label={t('outlet.powerNow', { power: readingText(power) })}>
              <span
                className={cn(
                  'text-3xl leading-none font-semibold tracking-tight tabular-nums transition-colors',
                  drawing ? 'text-foreground' : 'text-muted-foreground',
                )}
              >
                {power.value}
              </span>
              <span className="text-base text-muted-foreground">{power.unit}</span>
            </span>
            <span className="flex min-w-0 flex-col items-end text-right text-sm text-muted-foreground tabular-nums">
              {energyWh !== undefined && (
                <span className="truncate">
                  {t('outlet.energyTotal', { energy: readingText(energyReading(energyWh, language)) })}
                </span>
              )}
              {details.length > 0 && <span className="truncate">{details.join(' · ')}</span>}
            </span>
          </div>
        )}
        {onToggle === undefined && (
          <p className="flex min-w-0 items-center gap-1.5 text-sm text-muted-foreground">
            <Icon name="watch_only" className="size-4 shrink-0" />
            <span className="truncate">{t('outlet.watchOnly')}</span>
          </p>
        )}
      </WidgetFrame>
    </div>
  )
}
