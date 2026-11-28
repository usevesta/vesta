import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ChargingSchedule, ChargingScheduleFeatures, ChargingWindow, TimeOfDay } from '@/api/generated'
import { CommandButton } from '@/components/CommandButton'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { isPromiseLike, isSuccess, type CommandHandler } from '@/lib/command-gate'
import { useCommand } from '@/lib/use-command'
import { cn } from '@/lib/utils'
import {
  daysSummary,
  formatTime,
  isOvernight,
  newWindow,
  scheduleProblem,
  toggleDay,
  uniqueWindows,
  WEEK,
} from '@/widgets/ev-charger/schedule'

/** "22:00 – 6:00 next day · Monday to Friday", in the screen's language. */
function useWindowText() {
  const { t, i18n } = useTranslation()
  return (window: ChargingWindow) => {
    const times = {
      from: formatTime(window.start, i18n.language),
      to: formatTime(window.end, i18n.language),
    }
    const span = isOvernight(window) ? t('ev.hours.spanNextDay', times) : t('ev.hours.span', times)
    const summary = daysSummary(window.days)
    const days =
      summary.key === 'ev.hours.someDays'
        ? summary.days.map((day) => t(`ev.days.short.${day}`)).join(', ')
        : t(summary.key)
    return `${span} · ${days}`
  }
}

/** The hours, one line per window (read-only). */
export function ScheduleSummary({ windows }: { windows: readonly ChargingWindow[] }) {
  const text = useWindowText()
  return (
    <ul className="flex flex-col gap-0.5">
      {uniqueWindows(windows).map((window, index) => (
        <li key={index} className="text-base font-medium tabular-nums">
          {text(window)}
        </li>
      ))}
    </ul>
  )
}

function TimeField({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string
  value: TimeOfDay
  disabled: boolean
  onChange: (time: TimeOfDay) => void
}) {
  const id = useId()
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1">
      <label htmlFor={id} className="text-sm text-muted-foreground">
        {label}
      </label>
      <Input
        id={id}
        type="time"
        value={value}
        disabled={disabled}
        required
        className="h-14 rounded-xl px-3 text-2xl font-semibold tabular-nums md:text-2xl"
        onChange={(event) => {
          onChange(event.target.value)
        }}
      />
    </div>
  )
}

function WindowFields({
  window,
  number,
  weekdays,
  disabled,
  onChange,
  onRemove,
}: {
  window: ChargingWindow
  number: number
  weekdays: boolean
  disabled: boolean
  onChange: (window: ChargingWindow) => void
  onRemove?: () => void
}) {
  const { t } = useTranslation()
  const titleId = useId()
  return (
    <section aria-labelledby={titleId} className="flex flex-col gap-3 rounded-xl bg-muted/60 p-4">
      <div className="flex min-h-10 items-center justify-between gap-2">
        <h3 id={titleId} className="text-base font-medium">
          {t('ev.hours.window', { number })}
        </h3>
        {onRemove !== undefined && (
          <Button
            variant="ghost"
            className="h-auto min-h-12 gap-2 px-3 text-base"
            disabled={disabled}
            aria-label={t('ev.hours.removeLabel', { number })}
            onClick={onRemove}
          >
            <Icon name="remove" className="size-5" />
            {t('ev.hours.remove')}
          </Button>
        )}
      </div>
      <div className="flex gap-3">
        <TimeField
          label={t('ev.hours.from')}
          value={window.start}
          disabled={disabled}
          onChange={(start) => {
            onChange({ ...window, start })
          }}
        />
        <TimeField
          label={t('ev.hours.to')}
          value={window.end}
          disabled={disabled}
          onChange={(end) => {
            onChange({ ...window, end })
          }}
        />
      </div>
      {isOvernight(window) && (
        <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
          <Icon name="info" className="size-4 shrink-0" />
          {t('ev.hours.nextDay')}
        </p>
      )}
      {weekdays && (
        <div className="flex flex-col gap-1">
          <span className="text-sm text-muted-foreground">{t('ev.hours.days')}</span>
          <div role="group" aria-label={t('ev.hours.days')} className="grid grid-cols-7 gap-1.5">
            {WEEK.map((day) => {
              const on = window.days.includes(day)
              return (
                <Button
                  key={day}
                  variant={on ? 'default' : 'outline'}
                  aria-pressed={on}
                  aria-label={t(`ev.days.long.${day}`)}
                  disabled={disabled}
                  className="h-auto min-h-14 min-w-0 px-0 text-base capitalize"
                  onClick={() => {
                    onChange({ ...window, days: toggleDay(window.days, day) })
                  }}
                >
                  {t(`ev.days.short.${day}`)}
                </Button>
              )
            })}
          </div>
        </div>
      )}
    </section>
  )
}

function EditorBody({
  label,
  features,
  schedule,
  enabled,
  onSave,
  onDone,
}: Omit<ScheduleEditorProps, 'open' | 'onOpenChange'> & { onDone: () => void }) {
  const { t } = useTranslation()
  const switchId = useId()
  const [useHours, setUseHours] = useState(enabled)
  const [windows, setWindows] = useState<ChargingWindow[]>(() => {
    const known = uniqueWindows(schedule?.windows ?? [])
    return known.length > 0 ? known : [newWindow()]
  })
  // Closes once the charger has the hours; stays open (with the message)
  // when saving failed.
  const save = useCommand((next: ChargingSchedule) => {
    const result = onSave(next)
    if (!isPromiseLike(result)) {
      onDone()
      return undefined
    }
    return Promise.resolve(result).then((outcome) => {
      if (isSuccess(outcome)) onDone()
      return outcome
    })
  })
  const busy = save.busy
  const problem = scheduleProblem(windows, features)
  const days = (window: ChargingWindow) => (features.weekdays ? window : { ...window, days: [...WEEK] })

  return (
    <>
      <div className="flex flex-col gap-4" aria-busy={busy || undefined}>
        <label htmlFor={switchId} className="flex min-h-14 cursor-pointer items-center gap-3 rounded-xl">
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-base leading-tight font-medium">{t('ev.hours.useThem')}</span>
            <span className="text-sm leading-snug text-muted-foreground">{t('ev.hours.useThemHint')}</span>
          </span>
          <Switch id={switchId} size="lg" checked={useHours} disabled={busy} onCheckedChange={setUseHours} />
        </label>
        {windows.map((window, index) => (
          <WindowFields
            key={index}
            window={window}
            number={index + 1}
            weekdays={features.weekdays}
            disabled={busy}
            onChange={(next) => {
              setWindows(windows.map((other, i) => (i === index ? next : other)))
            }}
            onRemove={
              windows.length > 1
                ? () => {
                    setWindows(windows.filter((_, i) => i !== index))
                  }
                : undefined
            }
          />
        ))}
        {windows.length < features.max_windows && (
          <Button
            variant="outline"
            className="h-auto min-h-14 gap-2 text-base"
            disabled={busy}
            onClick={() => {
              setWindows([...windows, newWindow()])
            }}
          >
            <Icon name="add" className="size-5" />
            {t('ev.hours.add')}
          </Button>
        )}
        {problem !== null && (
          <p role="status" className="flex items-start gap-1.5 text-sm leading-snug text-warning">
            <Icon name="warning" className="mt-0.5 size-4 shrink-0" />
            {t(problem)}
          </p>
        )}
        {features.saving_needs_internet && (
          <p className="flex items-start gap-1.5 text-sm leading-snug text-muted-foreground">
            <Icon name="info" className="mt-0.5 size-4 shrink-0" />
            {t('ev.hours.needsInternet')}
          </p>
        )}
      </div>
      <DialogFooter className="gap-2 sm:gap-2">
        <Button variant="secondary" className="h-auto min-h-14 flex-1 text-lg" disabled={busy} onClick={onDone}>
          {t('common.cancel')}
        </Button>
        <CommandButton
          className="h-auto min-h-14 flex-1 gap-2 text-lg"
          disabled={problem !== null}
          busy={busy}
          icon="check"
          aria-label={t('ev.hours.changeLabel', { name: label })}
          onClick={() => {
            save.run({ enabled: useHours, windows: windows.map(days) })
          }}
        >
          {t('ev.hours.save')}
        </CommandButton>
      </DialogFooter>
    </>
  )
}

export interface ScheduleEditorProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The charger's name. */
  label: string
  features: ChargingScheduleFeatures
  /** The hours now, when known. */
  schedule?: ChargingSchedule
  /** The schedule is on now. */
  enabled: boolean
  /** Stores the hours in the charger. A promise keeps Save busy; the dialog closes once it worked. */
  onSave: CommandHandler<[ChargingSchedule]>
}

/**
 * Change the charging hours: from, to, days, on/off and Save. Big targets,
 * plain words; says when saving needs the internet. The charger keeps the
 * hours and follows them by itself.
 */
export function ScheduleEditor({ open, onOpenChange, ...props }: ScheduleEditorProps) {
  const { t } = useTranslation()
  const [saving, setSaving] = useState(false)
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // Not while the hours are on their way.
        if (next || !saving) onOpenChange(next)
      }}
    >
      <DialogContent
        className={cn('max-h-[94dvh] gap-5 overflow-y-auto p-6 sm:max-w-2xl')}
        showCloseButton={!saving}
        data-slot="schedule-editor"
      >
        <DialogHeader className="flex-row items-center gap-3 pr-10 text-left">
          <span className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground">
            <Icon name="schedule" className="size-6" />
          </span>
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <DialogTitle className="text-2xl">{t('ev.hours.title')}</DialogTitle>
            <DialogDescription className="text-base">{t('ev.hours.description')}</DialogDescription>
          </span>
        </DialogHeader>
        <EditorBody
          {...props}
          onSave={(schedule) => {
            setSaving(true)
            const result = props.onSave(schedule)
            if (!isPromiseLike(result)) {
              setSaving(false)
              return result
            }
            return Promise.resolve(result).finally(() => {
              setSaving(false)
            })
          }}
          onDone={() => {
            onOpenChange(false)
          }}
        />
      </DialogContent>
    </Dialog>
  )
}
