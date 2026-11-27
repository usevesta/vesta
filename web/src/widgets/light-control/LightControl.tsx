import { useId } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Switch } from '@/components/ui/switch'
import type { IconName } from '@/lib/icons'
import { LabeledSlider } from '@/widgets/parts/LabeledSlider'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface LightColorTemperature {
  kelvin: number
  minKelvin: number
  maxKelvin: number
}

export interface LightControlProps extends WidgetStatusProps {
  /** Name shown on the tile, e.g. "Living room light". */
  label: string
  /** Semantic icon. Default "light". */
  icon?: IconName
  on: boolean
  onToggle: (on: boolean) => void
  /** Brightness in % (1–100). Omit when the light cannot dim. */
  brightness?: number
  /** Dragging while off should switch the light on (handled upstream). */
  onBrightnessChange?: (brightness: number) => void
  /** Present when the light supports white tone (Kelvin). */
  colorTemperature?: LightColorTemperature
  onColorTemperatureChange?: (kelvin: number) => void
}

export function LightControl({
  label,
  icon = 'light',
  on,
  onToggle,
  brightness,
  onBrightnessChange,
  colorTemperature,
  onColorTemperatureChange,
  pending,
  unavailable = false,
}: LightControlProps) {
  const { t } = useTranslation()
  const switchId = useId()
  const percent = (value: number) => t('common.percent', { value: Math.round(value) })

  const subtitle = on
    ? brightness === undefined
      ? t('common.on')
      : `${t('common.on')} · ${percent(brightness)}`
    : t('common.off')

  return (
    <WidgetFrame
      icon={icon}
      title={label}
      subtitle={subtitle}
      active={on}
      pending={pending}
      unavailable={unavailable}
      headerControlId={switchId}
      headerAction={
        <Switch
          id={switchId}
          size="lg"
          checked={on}
          disabled={unavailable}
          aria-label={on ? t('widget.turnOff', { name: label }) : t('widget.turnOn', { name: label })}
          onCheckedChange={onToggle}
        />
      }
    >
      {brightness !== undefined && onBrightnessChange !== undefined && (
        <LabeledSlider
          label={t('light.brightness')}
          value={brightness}
          min={1}
          max={100}
          disabled={unavailable}
          formatValue={percent}
          onCommit={onBrightnessChange}
        />
      )}
      {colorTemperature !== undefined && onColorTemperatureChange !== undefined && (
        <LabeledSlider
          label={t('light.whiteTone')}
          value={colorTemperature.kelvin}
          min={colorTemperature.minKelvin}
          max={colorTemperature.maxKelvin}
          step={50}
          disabled={unavailable}
          onCommit={onColorTemperatureChange}
          startLabel={
            <>
              <Icon name="warm" className="size-4" />
              {t('light.warm')}
            </>
          }
          endLabel={
            <>
              {t('light.cool')}
              <Icon name="cool" className="size-4" />
            </>
          }
        />
      )}
    </WidgetFrame>
  )
}
