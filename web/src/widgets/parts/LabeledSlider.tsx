import { useState, type ReactNode } from 'react'

import { Slider } from '@/components/ui/slider'

export interface LabeledSliderProps {
  label: string
  value: number
  min: number
  max: number
  step?: number
  disabled?: boolean
  /** Shown next to the label while idle and while dragging. */
  formatValue?: (value: number) => string
  /** Called once when the finger lifts, never while dragging. */
  onCommit: (value: number) => void
  startLabel?: ReactNode
  endLabel?: ReactNode
}

/** Slider that previews locally while dragging and commits on release. */
export function LabeledSlider({
  label,
  value,
  min,
  max,
  step = 1,
  disabled = false,
  formatValue,
  onCommit,
  startLabel,
  endLabel,
}: LabeledSliderProps) {
  const [draft, setDraft] = useState<number | null>(null)
  const shown = draft ?? value
  const valueText = formatValue?.(shown)

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline justify-between gap-2 text-sm">
        <span className="text-muted-foreground">{label}</span>
        {valueText !== undefined && <span className="font-medium tabular-nums">{valueText}</span>}
      </div>
      <Slider
        value={[shown]}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        thumbLabel={label}
        thumbValueText={valueText}
        onValueChange={(values) => {
          const next = values[0]
          if (next !== undefined) setDraft(next)
        }}
        onValueCommit={(values) => {
          const next = values[0]
          setDraft(null)
          if (next !== undefined && next !== value) onCommit(next)
        }}
      />
      {(startLabel !== undefined || endLabel !== undefined) && (
        <div className="flex justify-between text-sm text-muted-foreground">
          <span className="inline-flex items-center gap-1">{startLabel}</span>
          <span className="inline-flex items-center gap-1">{endLabel}</span>
        </div>
      )}
    </div>
  )
}
