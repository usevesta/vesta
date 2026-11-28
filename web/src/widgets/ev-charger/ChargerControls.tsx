import { useId, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { ChargingSchedule, ChargingScheduleFeatures, EvChargeMode } from '@/api/generated'
import { CommandButton } from '@/components/CommandButton'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import type { CommandHandler } from '@/lib/command-gate'
import type { IconName } from '@/lib/icons'
import { useCommand } from '@/lib/use-command'
import { cn } from '@/lib/utils'
import { formatAmps } from '@/widgets/ev-charger/energy'
import { ScheduleEditor, ScheduleSummary } from '@/widgets/ev-charger/ScheduleEditor'
import { LabeledSlider } from '@/widgets/parts/LabeledSlider'

export interface CurrentRange {
  readonly min: number
  readonly max: number
}

export interface ChargerControlsProps {
  label: string
  disabled: boolean
  currentA?: number
  currentRange?: CurrentRange
  /** The charger sets the current by itself right now (its own power control). */
  autoCurrent?: boolean
  /** Command callbacks: a promise they return keeps the control busy. */
  onCurrentChange?: CommandHandler<[number]>
  mode?: EvChargeMode
  /** Modes that can be picked here. */
  modes: readonly EvChargeMode[]
  onModeChange?: CommandHandler<[EvChargeMode]>
  locked?: boolean
  /** Unlocking is confirmed first; the callback runs after that. */
  onLockChange?: CommandHandler<[boolean]>
  scheduleEnabled?: boolean
  onScheduleChange?: CommandHandler<[boolean]>
  /** The charger's hours, when known. */
  chargingSchedule?: ChargingSchedule
  /** What the charger's hours allow (absent: they are set on the charger). */
  scheduleFeatures?: ChargingScheduleFeatures
  /** Present when the hours can be changed from here. */
  onChargingScheduleChange?: CommandHandler<[ChargingSchedule]>
}

function SettingRow({
  icon,
  title,
  hint,
  control,
  controlId,
}: {
  icon: IconName
  title: string
  hint?: ReactNode
  control: ReactNode
  controlId?: string
}) {
  const body = (
    <>
      <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
        <Icon name={icon} className="size-5" />
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-base leading-tight font-medium">{title}</span>
        {hint !== undefined && <span className="text-sm leading-snug text-muted-foreground">{hint}</span>}
      </span>
      {control}
    </>
  )
  const className = 'flex min-h-14 items-center gap-3 rounded-xl py-1'
  return controlId === undefined ? (
    <div className={className}>{body}</div>
  ) : (
    <label htmlFor={controlId} className={cn(className, 'cursor-pointer')}>
      {body}
    </label>
  )
}

function StateText({ children }: { children: ReactNode }) {
  return <span className="text-base font-medium text-muted-foreground">{children}</span>
}

/** Current, mode, lock and schedule: each only when the charger offers it. */
export function ChargerControls({
  label,
  disabled,
  currentA,
  currentRange,
  autoCurrent = false,
  onCurrentChange,
  mode,
  modes,
  onModeChange,
  locked,
  onLockChange,
  scheduleEnabled,
  onScheduleChange,
  chargingSchedule,
  scheduleFeatures,
  onChargingScheduleChange,
}: ChargerControlsProps) {
  const { t, i18n } = useTranslation()
  const lockId = useId()
  const scheduleId = useId()
  const [confirmUnlock, setConfirmUnlock] = useState(false)
  const [editingHours, setEditingHours] = useState(false)
  const setMode = useCommand(onModeChange)
  const lock = useCommand(onLockChange)
  const schedule = useCommand(onScheduleChange)
  const amps = (value: number) => t('ev.amps', { value: formatAmps(value, i18n.language) })

  const canSetCurrent = onCurrentChange !== undefined && currentRange !== undefined && !autoCurrent
  const shownCurrent =
    currentA === undefined || currentRange === undefined
      ? currentA
      : Math.min(currentRange.max, Math.max(currentRange.min, currentA))
  const pickable = mode === undefined || modes.includes(mode)

  return (
    <div className="flex flex-col gap-4">
      {canSetCurrent ? (
        <LabeledSlider
          label={t('ev.currentLabel')}
          value={shownCurrent ?? currentRange.min}
          min={currentRange.min}
          max={currentRange.max}
          step={1}
          disabled={disabled}
          formatValue={amps}
          onCommit={onCurrentChange}
          startLabel={amps(currentRange.min)}
          endLabel={amps(currentRange.max)}
        />
      ) : (
        currentA !== undefined &&
        autoCurrent && (
          <SettingRow
            icon="meter"
            title={t('ev.currentLabel')}
            hint={t('ev.autoCurrent')}
            control={<span className="text-lg font-semibold tabular-nums">{amps(currentA)}</span>}
          />
        )
      )}

      {modes.length > 0 && onModeChange !== undefined && pickable ? (
        <div className="flex flex-col gap-2">
          <span className="text-sm text-muted-foreground">{t('ev.mode')}</span>
          <div role="group" aria-label={t('ev.mode')} className="grid auto-cols-fr grid-flow-col gap-2">
            {modes.map((option) => {
              const selected = option === mode
              return (
                <CommandButton
                  key={option}
                  variant={selected ? 'default' : 'secondary'}
                  aria-pressed={selected}
                  // One mode at a time: the others wait for this one.
                  disabled={disabled || setMode.busy}
                  busy={setMode.busyWith?.[0] === option}
                  className="h-auto min-h-16 flex-col items-start justify-center gap-0.5 px-3 py-2 pr-7 text-left whitespace-normal"
                  onClick={() => {
                    if (!selected) setMode.run(option)
                  }}
                >
                  <span className="text-base leading-tight font-medium">{t(`ev.modes.${option}`)}</span>
                  <span className={cn('text-xs leading-snug', selected ? 'opacity-80' : 'text-muted-foreground')}>
                    {t(`ev.modeHints.${option}`)}
                  </span>
                </CommandButton>
              )
            })}
          </div>
        </div>
      ) : (
        mode !== undefined && (
          <SettingRow
            icon="more_controls"
            title={t('ev.mode')}
            hint={modes.length > 0 ? t('ev.modeElsewhere') : t(`ev.modeHints.${mode}`)}
            control={<span className="text-base font-medium">{t(`ev.modes.${mode}`)}</span>}
          />
        )
      )}

      {(locked !== undefined || scheduleEnabled !== undefined) && (
        <div className="flex flex-col gap-1">
          {locked !== undefined && (
            <SettingRow
              icon={locked ? 'lock' : 'unlock'}
              title={t('ev.lock')}
              hint={t('ev.lockHint')}
              controlId={onLockChange === undefined ? undefined : lockId}
              control={
                onLockChange === undefined ? (
                  <StateText>{locked ? t('ev.locked') : t('ev.unlocked')}</StateText>
                ) : (
                  <Switch
                    id={lockId}
                    size="lg"
                    checked={locked}
                    disabled={disabled}
                    busy={lock.busy}
                    aria-label={t('ev.lock')}
                    onCheckedChange={(next) => {
                      if (next) lock.run(true)
                      else setConfirmUnlock(true)
                    }}
                  />
                )
              }
            />
          )}
          {scheduleEnabled !== undefined && (
            <SettingRow
              icon="schedule"
              title={t('ev.schedule')}
              hint={scheduleFeatures === undefined ? t('ev.scheduleHint') : undefined}
              controlId={onScheduleChange === undefined ? undefined : scheduleId}
              control={
                onScheduleChange === undefined ? (
                  <StateText>{scheduleEnabled ? t('common.on') : t('common.off')}</StateText>
                ) : (
                  <Switch
                    id={scheduleId}
                    size="lg"
                    checked={scheduleEnabled}
                    disabled={disabled}
                    busy={schedule.busy}
                    aria-label={t('ev.schedule')}
                    onCheckedChange={schedule.run}
                  />
                )
              }
            />
          )}
          {scheduleFeatures !== undefined && (
            <div className="flex flex-col gap-2 pl-13" data-slot="charging-hours">
              {chargingSchedule === undefined ? (
                <p className="text-sm leading-snug text-muted-foreground">{t('ev.hours.unknown')}</p>
              ) : (
                <>
                  <ScheduleSummary windows={chargingSchedule.windows} />
                  {!scheduleFeatures.reports_windows && (
                    <p className="text-sm leading-snug text-muted-foreground">{t('ev.hours.savedHere')}</p>
                  )}
                </>
              )}
              {scheduleFeatures.editable && onChargingScheduleChange !== undefined && (
                <Button
                  variant="outline"
                  className="h-auto min-h-14 gap-2 self-start px-4 text-base"
                  disabled={disabled}
                  aria-label={t('ev.hours.changeLabel', { name: label })}
                  onClick={() => {
                    setEditingHours(true)
                  }}
                >
                  <Icon name="edit" className="size-5" />
                  {t('ev.hours.change')}
                </Button>
              )}
            </div>
          )}
        </div>
      )}

      {scheduleFeatures?.editable === true && onChargingScheduleChange !== undefined && (
        <ScheduleEditor
          open={editingHours}
          onOpenChange={setEditingHours}
          label={label}
          features={scheduleFeatures}
          schedule={chargingSchedule}
          enabled={scheduleEnabled ?? chargingSchedule?.enabled ?? false}
          onSave={onChargingScheduleChange}
        />
      )}

      <ConfirmDialog
        open={confirmUnlock}
        onOpenChange={setConfirmUnlock}
        icon="unlock"
        title={t('ev.unlockTitle', { name: label })}
        description={t('ev.unlockBody')}
        confirmLabel={t('ev.unlock')}
        // The dialog stays open (busy) until the charger answered.
        onConfirm={() => onLockChange?.(false)}
      />
    </div>
  )
}
