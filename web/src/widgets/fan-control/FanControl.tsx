import { useId } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import type { IconName } from '@/lib/icons'
import { LabeledSlider } from '@/widgets/parts/LabeledSlider'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export type FanDirection = 'forward' | 'reverse'

/** Up to this many steps are shown as buttons; more use a slider. */
const MAX_SPEED_BUTTONS = 6

export interface FanControlProps extends WidgetStatusProps {
  label: string
  /** Semantic icon. Default "fan". */
  icon?: IconName
  on: boolean
  onToggle: (on: boolean) => void
  /** Current speed, 1..speedSteps. */
  speed: number
  /** Number of speeds the fan offers (>= 1). */
  speedSteps: number
  onSpeedChange: (speed: number) => void
  /** Present only when the fan can reverse. */
  direction?: FanDirection
  onDirectionChange?: (direction: FanDirection) => void
}

export function FanControl({
  label,
  icon = 'fan',
  on,
  onToggle,
  speed,
  speedSteps,
  onSpeedChange,
  direction,
  onDirectionChange,
  pending,
  unavailable = false,
}: FanControlProps) {
  const { t } = useTranslation()
  const switchId = useId()
  const steps = Math.max(1, Math.floor(speedSteps))
  const speeds = Array.from({ length: steps }, (_, index) => index + 1)

  return (
    <WidgetFrame
      icon={icon}
      title={label}
      subtitle={on ? t('fan.speedStep', { speed }) : t('common.off')}
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
      {steps <= MAX_SPEED_BUTTONS ? (
        <div role="group" aria-label={t('fan.speed')} className="flex gap-2">
          {speeds.map((step) => {
            const selected = on && step === speed
            return (
              <Button
                key={step}
                variant={selected ? 'default' : 'secondary'}
                className="h-auto min-h-14 flex-1 text-lg tabular-nums"
                aria-pressed={selected}
                aria-label={t('fan.speedStep', { speed: step })}
                disabled={unavailable}
                onClick={() => {
                  onSpeedChange(step)
                }}
              >
                {step}
              </Button>
            )
          })}
        </div>
      ) : (
        <LabeledSlider
          label={t('fan.speed')}
          value={speed}
          min={1}
          max={steps}
          disabled={unavailable}
          formatValue={String}
          onCommit={onSpeedChange}
        />
      )}
      {direction !== undefined && onDirectionChange !== undefined && (
        <div role="group" aria-label={t('fan.direction')} className="flex gap-2">
          {(['forward', 'reverse'] as const).map((option) => (
            <Button
              key={option}
              variant={direction === option ? 'default' : 'secondary'}
              className="h-auto min-h-14 flex-1 gap-2 text-base"
              aria-pressed={direction === option}
              disabled={unavailable}
              onClick={() => {
                onDirectionChange(option)
              }}
            >
              <Icon name={option === 'forward' ? 'direction_forward' : 'direction_reverse'} className="size-5" />
              {option === 'forward' ? t('fan.forward') : t('fan.reverse')}
            </Button>
          ))}
        </div>
      )}
    </WidgetFrame>
  )
}
