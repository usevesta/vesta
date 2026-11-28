import { useEffect, useRef, useState, type ReactNode } from 'react'

import { BusyIcon } from '@/components/BusyIcon'
import { Slider } from '@/components/ui/slider'
import type { CommandHandler } from '@/lib/command-gate'
import { useCommand } from '@/lib/use-command'

/** Arrow keys: send once the person stopped pressing for this long. */
export const KEYBOARD_SETTLE_MS = 400

export interface LabeledSliderProps {
  label: string
  value: number
  min: number
  max: number
  step?: number
  disabled?: boolean
  /** Shown next to the label while idle and while dragging. */
  formatValue?: (value: number) => string
  /**
   * Called once when the finger lifts (or the arrow keys rest), never
   * while dragging. A promise it returns keeps the slider busy.
   */
  onCommit: CommandHandler<[number]>
  startLabel?: ReactNode
  endLabel?: ReactNode
}

/**
 * Slider that previews locally while dragging and sends on release. While
 * the value it sent is on its way, it is busy: disabled, with a spinner
 * next to the value.
 */
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
  const command = useCommand(onCommit)
  const pointer = useRef(false)
  const settle = useRef<ReturnType<typeof setTimeout> | null>(null)
  const shown = draft ?? value
  const valueText = formatValue?.(shown)

  useEffect(
    () => () => {
      if (settle.current !== null) clearTimeout(settle.current)
    },
    [],
  )

  const commit = (next: number) => {
    setDraft(null)
    if (next !== value) command.run(next)
  }

  return (
    <div className="flex flex-col gap-1" aria-busy={command.busy || undefined}>
      <div className="flex items-baseline justify-between gap-2 text-sm">
        <span className="text-muted-foreground">{label}</span>
        {(valueText !== undefined || command.busy) && (
          <span className="inline-flex items-center gap-1.5 font-medium tabular-nums">
            <BusyIcon busy={command.busy} className="size-4 text-muted-foreground" />
            {valueText}
          </span>
        )}
      </div>
      <Slider
        value={[shown]}
        min={min}
        max={max}
        step={step}
        disabled={disabled || command.busy}
        // Busy is not unavailable: keep it readable.
        className={command.busy ? 'data-disabled:opacity-100' : undefined}
        thumbLabel={label}
        thumbValueText={valueText}
        onPointerDown={() => {
          pointer.current = true
        }}
        onValueChange={(values) => {
          const next = values[0]
          if (next !== undefined) setDraft(next)
        }}
        onValueCommit={(values) => {
          const next = values[0]
          if (next === undefined) return
          if (settle.current !== null) clearTimeout(settle.current)
          settle.current = null
          if (pointer.current) {
            pointer.current = false
            commit(next)
            return
          }
          // Arrow keys commit every step: send the last one only.
          setDraft(next)
          settle.current = setTimeout(() => {
            settle.current = null
            commit(next)
          }, KEYBOARD_SETTLE_MS)
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
