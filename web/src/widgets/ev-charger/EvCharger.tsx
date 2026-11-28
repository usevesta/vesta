import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { EvChargeMode, EvChargerStatus, EvChargerWaitReason } from '@/api/generated'
import { CommandButton } from '@/components/CommandButton'
import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { CommandHandler } from '@/lib/command-gate'
import type { IconName } from '@/lib/icons'
import { useCommand } from '@/lib/use-command'
import { useElementWidth } from '@/lib/use-element-width'
import { cn } from '@/lib/utils'
import { ChargerControls, type CurrentRange } from '@/widgets/ev-charger/ChargerControls'
import {
  carryingPhases,
  formatAmps,
  formatKw,
  formatKwh,
  headroom,
  splitDuration,
  statusKey,
  type EnergyReadings,
  type Headroom,
} from '@/widgets/ev-charger/energy'
import { EnergyFlow } from '@/widgets/ev-charger/EnergyFlow'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

/** Narrower than this, a one-row tile shows only the status and the power. */
const STRIP_MIN_PX = 400
/** From this width on, a one-row tile has room for everything, side by side. */
const INLINE_MIN_PX = 860
/** Details side by side (diagram | session and controls) from this width. */
const SIDE_BY_SIDE_PX = 700

export interface EvChargerProps extends WidgetStatusProps {
  /** e.g. "Car charger". */
  label: string
  /** Semantic icon. Default "ev_charger". */
  icon?: IconName
  /** Rows the tile spans. Two or more show the diagram and every control. */
  rows?: number
  /** Unknown until the charger reported. */
  status?: EvChargerStatus
  waitingFor?: EvChargerWaitReason
  /** Power going to the car, W. */
  powerW?: number
  sessionEnergyWh?: number
  sessionDurationS?: number
  /** The current the charger offers the car (its setpoint), A. */
  currentA?: number
  /** Measured current per phase, A. */
  measuredA?: readonly number[]
  currentRange?: CurrentRange
  /** The charger's own power control sets the current right now. */
  autoCurrent?: boolean
  mode?: EvChargeMode
  /** Modes that can be picked here (empty: none). */
  modes?: readonly EvChargeMode[]
  locked?: boolean
  scheduleEnabled?: boolean
  /** The home's meters around the charger (the car's power comes from `powerW`). */
  readings?: EnergyReadings
  /**
   * Present only when the charger can pause and resume. Command callbacks:
   * a promise they return keeps the control busy.
   */
  onPause?: CommandHandler
  onResume?: CommandHandler
  onCurrentChange?: CommandHandler<[number]>
  onModeChange?: CommandHandler<[EvChargeMode]>
  /** Called after the person confirmed unlocking. */
  onLockChange?: CommandHandler<[boolean]>
  onScheduleChange?: CommandHandler<[boolean]>
}

function Readout({ watts, size = 'lg' }: { watts: number | undefined; size?: 'lg' | 'xl' }) {
  const { t, i18n } = useTranslation()
  return (
    <span className="flex items-baseline gap-1.5">
      <span
        className={cn(
          'leading-none font-semibold tracking-tight tabular-nums',
          size === 'xl' ? 'text-5xl' : 'text-4xl',
        )}
      >
        {watts === undefined ? '–' : formatKw(watts, i18n.language)}
      </span>
      <span className="text-base text-muted-foreground">{t('ev.kwUnit')}</span>
    </span>
  )
}

/** A thin bar; a light sweeps through it while the car charges. */
function ChargeBar({ charging }: { charging: boolean }) {
  return (
    <span
      aria-hidden
      className={cn(
        'energy-layer relative block h-1.5 w-full overflow-hidden rounded-full transition-colors duration-500',
        charging ? 'bg-energy-car/20' : 'bg-muted',
      )}
    >
      {charging && (
        <span className="energy-sweep absolute inset-y-0 left-0 w-2/5 rounded-full bg-linear-to-r from-transparent via-energy-car to-transparent" />
      )}
    </span>
  )
}

function Stat({ label, value, detail }: { label: string; value: ReactNode; detail?: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 rounded-xl bg-muted/60 px-3 py-2.5">
      <span className="truncate text-xs font-medium tracking-wide text-muted-foreground uppercase">{label}</span>
      <span className="truncate text-xl leading-tight font-semibold tracking-tight tabular-nums">{value}</span>
      {detail !== undefined && <span className="truncate text-xs text-muted-foreground tabular-nums">{detail}</span>}
    </div>
  )
}

function useSession(props: EvChargerProps) {
  const { t, i18n } = useTranslation()
  const { sessionEnergyWh, sessionDurationS, currentA, measuredA } = props
  const energy = sessionEnergyWh === undefined ? '–' : t('ev.kwh', { value: formatKwh(sessionEnergyWh, i18n.language) })
  let time = '–'
  if (sessionDurationS !== undefined) {
    const { hours, minutes } = splitDuration(sessionDurationS)
    time = hours > 0 ? t('ev.duration.hoursMinutes', { hours, minutes }) : t('ev.duration.minutes', { minutes })
  }
  const phases = carryingPhases(measuredA)
  const amps = (value: number) => t('ev.amps', { value: formatAmps(value, i18n.language) })
  const now =
    phases.length === 0 ? amps(0) : phases.length === 1 ? amps(phases[0] ?? 0) : phases.map((a) => formatAmps(a, i18n.language)).join(' · ')
  const set = currentA === undefined ? undefined : t('ev.setTo', { value: amps(currentA) })
  return { energy, time, now, set }
}

function HeadroomBar({ room }: { room: Headroom }) {
  const { t, i18n } = useTranslation()
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between gap-2 text-sm">
        <span className="text-muted-foreground">{t('ev.contracted')}</span>
        <span className="font-medium tabular-nums">
          {t('ev.contractedValue', {
            used: formatKw(room.usedW, i18n.language),
            limit: formatKw(room.limitW, i18n.language),
          })}
        </span>
      </div>
      <span className="energy-layer block h-2 overflow-hidden rounded-full bg-muted">
        <span
          className={cn(
            'block h-full origin-left rounded-full transition-[transform,background-color] duration-700',
            room.nearLimit ? 'bg-energy-limit' : 'bg-energy-house/60',
          )}
          style={{ transform: `scaleX(${String(room.ratio)})` }}
        />
      </span>
      {room.nearLimit && (
        <p className="flex items-start gap-1.5 text-sm leading-snug text-energy-limit">
          <Icon name="info" className="mt-0.5 size-4 shrink-0" />
          {t('ev.nearLimit')}
        </p>
      )}
    </div>
  )
}

function hasMeters(readings: EnergyReadings | undefined): boolean {
  if (readings === undefined) return false
  const { gridW, solarW, batteryW, houseW } = readings
  return [gridW, solarW, batteryW, houseW].some((value) => value !== undefined)
}

/**
 * The diagram, the session and the controls: a big tile, or the dialog.
 * `settings: 'current'` keeps a stacked tile short enough for a wall
 * screen: only the charging current, the rest is in the dialog.
 */
function ChargerDetails({ settings = 'all', ...props }: EvChargerProps & { settings?: 'all' | 'current' }) {
  const { t } = useTranslation()
  const [ref, width] = useElementWidth()
  const { readings, powerW, unavailable = false, status } = props
  const flowReadings: EnergyReadings = { ...readings, ...(powerW === undefined ? {} : { carW: powerW }) }
  const room = headroom(flowReadings)
  const session = useSession(props)
  const sideBySide = width !== null && width >= SIDE_BY_SIDE_PX
  const charging = status === 'charging'

  const picture = hasMeters(readings) ? (
    <EnergyFlow
      readings={flowReadings}
      frozen={unavailable}
      className={cn('mx-auto max-w-[34rem]', !sideBySide && 'max-h-60')}
    />
  ) : (
    <div className="flex flex-col gap-3 py-2">
      <Readout watts={powerW} size="xl" />
      <ChargeBar charging={charging && !unavailable} />
    </div>
  )

  return (
    <div
      ref={ref}
      className={cn(
        sideBySide ? 'grid grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)] items-center gap-8' : 'flex flex-col gap-5',
      )}
    >
      <section className="flex flex-col gap-4" aria-label={t('ev.flowTitle')}>
        {picture}
        {room !== null && <HeadroomBar room={room} />}
      </section>
      <section className="flex flex-col gap-5">
        <div className="flex flex-col gap-2">
          <h3 className="text-sm text-muted-foreground">{t('ev.session')}</h3>
          <div className="grid grid-cols-3 gap-2">
            <Stat label={t('ev.energy')} value={session.energy} />
            <Stat label={t('ev.time')} value={session.time} />
            <Stat label={t('ev.now')} value={session.now} detail={session.set} />
          </div>
        </div>
        <ChargerControls
          label={props.label}
          disabled={unavailable}
          currentA={props.currentA}
          currentRange={props.currentRange}
          autoCurrent={props.autoCurrent}
          onCurrentChange={props.onCurrentChange}
          {...(settings === 'all'
            ? {
                mode: props.mode,
                modes: props.modes ?? [],
                onModeChange: props.onModeChange,
                locked: props.locked,
                onLockChange: props.onLockChange,
                scheduleEnabled: props.scheduleEnabled,
                onScheduleChange: props.onScheduleChange,
              }
            : { modes: [] })}
        />
      </section>
    </div>
  )
}

/**
 * The car charger: where the home's power goes, how the charge is going,
 * and the charger's controls. One row shows the status and the power (and
 * opens everything in a dialog); two rows, or a full-width tile, show it
 * all. Words are plain and the charger's brand never appears.
 */
export function EvCharger(props: EvChargerProps) {
  const { t, i18n } = useTranslation()
  const [ref, width] = useElementWidth()
  const [open, setOpen] = useState(false)
  const {
    label,
    icon = 'ev_charger',
    rows = 2,
    status,
    waitingFor,
    powerW,
    locked,
    pending,
    unavailable = false,
    onPause,
    onResume,
  } = props

  const inline = rows >= 2 || (width !== null && width >= INLINE_MIN_PX)
  // A stacked big tile shows the current only; the dialog has the rest.
  const stacked = inline && (width === null || width < SIDE_BY_SIDE_PX)
  const hasMoreSettings =
    props.mode !== undefined || props.locked !== undefined || props.scheduleEnabled !== undefined
  const withDialog = !inline || (stacked && hasMoreSettings)
  const glance = !inline && width !== null && width < STRIP_MIN_PX
  const charging = status === 'charging'
  const key = statusKey(status, waitingFor, locked)
  const power = t('ev.kw', { value: formatKw(powerW ?? 0, i18n.language) })
  const subtitle =
    key === null
      ? undefined
      : key === 'ev.status.charging' && !inline
        ? t('ev.status.chargingPlain')
        : t(key, { power })

  const paused = status === 'paused'
  const primary = paused ? onResume : onPause
  // One gate for pause and resume (the same button), on the tile and in the dialog.
  const pauseResume = useCommand(primary)
  const primaryButton = (
    <CommandButton
      variant={paused ? 'default' : 'secondary'}
      className="h-auto min-h-14 shrink-0 gap-2 px-4 text-base"
      disabled={unavailable || status === undefined}
      busy={pauseResume.busy}
      icon={paused ? 'resume' : 'pause'}
      aria-label={paused ? t('ev.resumeLabel', { name: label }) : t('ev.pauseLabel', { name: label })}
      onClick={() => {
        pauseResume.run()
      }}
    >
      {paused ? t('ev.resume') : t('ev.pause')}
    </CommandButton>
  )
  const primaryAction = primary === undefined || glance ? null : primaryButton
  const more = !withDialog ? null : (
    <Button
      variant="ghost"
      className="size-14 shrink-0 rounded-xl"
      aria-label={t('ev.moreLabel', { name: label })}
      onClick={() => {
        setOpen(true)
      }}
    >
      <Icon name="more_controls" className="size-6" />
    </Button>
  )
  const session = useSession(props)

  let body: ReactNode
  if (inline) {
    body = <ChargerDetails {...props} settings={stacked ? 'current' : 'all'} />
  } else if (glance) {
    body = (
      <div className="flex flex-col gap-2">
        <Readout watts={powerW} />
        <ChargeBar charging={charging && !unavailable} />
      </div>
    )
  } else {
    body = (
      <div className="flex items-end gap-6">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Readout watts={powerW} />
          <ChargeBar charging={charging && !unavailable} />
        </div>
        <dl className="flex shrink-0 gap-5 text-right">
          <div className="flex flex-col">
            <dt className="text-xs text-muted-foreground">{t('ev.energy')}</dt>
            <dd className="text-lg font-semibold tabular-nums">{session.energy}</dd>
          </div>
          <div className="flex flex-col">
            <dt className="text-xs text-muted-foreground">{t('ev.time')}</dt>
            <dd className="text-lg font-semibold tabular-nums">{session.time}</dd>
          </div>
        </dl>
      </div>
    )
  }

  return (
    <div ref={ref} className="h-full min-w-0" data-slot="ev-charger" data-status={status}>
      <WidgetFrame
        icon={icon}
        title={label}
        subtitle={subtitle}
        active={charging}
        pending={pending}
        unavailable={unavailable}
        headerAction={
          primaryAction === null && more === null ? undefined : (
            <span className="flex shrink-0 items-center gap-1">
              {primaryAction}
              {more}
            </span>
          )
        }
      >
        {body}
      </WidgetFrame>
      {withDialog && (
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogContent className="max-h-[94dvh] gap-5 overflow-y-auto p-6 sm:max-w-5xl">
            <DialogHeader className="flex-row items-center gap-3 pr-10 text-left">
              <span
                className={cn(
                  'flex size-12 shrink-0 items-center justify-center rounded-xl',
                  charging ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground',
                )}
              >
                <Icon name={icon} className="size-6" />
              </span>
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <DialogTitle className="text-2xl">{label}</DialogTitle>
                <DialogDescription className="text-base">{key === null ? '' : t(key, { power })}</DialogDescription>
              </span>
              {primary === undefined ? null : primaryButton}
            </DialogHeader>
            <ChargerDetails {...props} />
          </DialogContent>
        </Dialog>
      )}
    </div>
  )
}
