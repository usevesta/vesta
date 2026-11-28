import { useTranslation } from 'react-i18next'

import type { RobotVacuumStatus } from '@/api/generated'
import { CommandButton } from '@/components/CommandButton'
import { Icon } from '@/components/Icon'
import type { CommandHandler } from '@/lib/command-gate'
import type { IconName } from '@/lib/icons'
import { useCommand } from '@/lib/use-command'
import { useElementWidth } from '@/lib/use-element-width'
import { cn } from '@/lib/utils'
import { availableActions, isBusy, splitMinutes, statusKey } from '@/widgets/robot-vacuum/robot'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface RobotVacuumProps extends WidgetStatusProps {
  /** e.g. "Robot vacuum". */
  label: string
  /** Semantic icon. Default "robot_vacuum". */
  icon?: IconName
  /** Unknown until the robot reported. */
  status?: RobotVacuumStatus
  batteryPercent?: number
  batteryLow?: boolean
  /** The current (or last) cleaning. */
  cleanedAreaM2?: number
  cleaningDurationS?: number
  /**
   * Each present only when the robot can do it from here. A promise they
   * return keeps the button busy.
   */
  onStart?: CommandHandler
  onPause?: CommandHandler
  onReturnToDock?: CommandHandler
}

function Battery({ percent, low, charging }: { percent: number; low: boolean; charging: boolean }) {
  const { t } = useTranslation()
  const icon: IconName = charging ? 'charging' : low ? 'battery_low' : percent >= 90 ? 'battery_full' : 'battery'
  return (
    <span
      className={cn(
        'flex shrink-0 items-center gap-1.5 rounded-full border px-3 py-1.5 text-base font-medium tabular-nums',
        low ? 'border-destructive/40 bg-destructive/10 text-destructive' : 'border-border text-muted-foreground',
      )}
      role="status"
      aria-label={low ? t('robot.batteryLowLabel', { percent }) : t('robot.batteryLabel', { percent })}
    >
      <Icon name={icon} className="size-5" />
      {t('common.percent', { value: percent })}
    </span>
  )
}

/** Narrower than this, the buttons show only their icons. */
const LABELS_MIN_PX = 420

/**
 * A robot vacuum: what it is doing in plain words, its battery, its last
 * clean, and start / pause / back to its base when it can be controlled
 * from here. A robot that is only watched shows no buttons.
 */
export function RobotVacuum({
  label,
  icon = 'robot_vacuum',
  status,
  batteryPercent,
  batteryLow = false,
  cleanedAreaM2,
  cleaningDurationS,
  onStart,
  onPause,
  onReturnToDock,
  pending,
  unavailable = false,
}: RobotVacuumProps) {
  const { t, i18n } = useTranslation()
  const [ref, width] = useElementWidth()
  const start = useCommand(onStart)
  const pause = useCommand(onPause)
  const dock = useCommand(onReturnToDock)
  const compact = width !== null && width < LABELS_MIN_PX
  const key = statusKey(status)
  const actions = availableActions(status)
  const active = isBusy(status)
  const working = status === 'cleaning'

  const facts: string[] = []
  if (cleanedAreaM2 !== undefined) {
    const area = new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(cleanedAreaM2)
    facts.push(t('robot.area', { value: area }))
  }
  if (cleaningDurationS !== undefined) {
    const { hours, minutes } = splitMinutes(cleaningDurationS)
    facts.push(hours > 0 ? t('robot.hoursMinutes', { hours, minutes }) : t('robot.minutes', { minutes }))
  }

  const buttonClass = 'h-auto min-h-14 min-w-0 flex-1 gap-2 px-3 text-base'
  const text = (words: string) => (compact ? <span className="sr-only">{words}</span> : words)
  const buttons = [
    onStart === undefined ? null : (
      <CommandButton
        key="start"
        variant={actions.resumes ? 'default' : 'secondary'}
        className={buttonClass}
        disabled={unavailable || !actions.start}
        busy={start.busy}
        icon="resume"
        aria-label={actions.resumes ? t('robot.resumeLabel', { name: label }) : t('robot.startLabel', { name: label })}
        onClick={() => {
          start.run()
        }}
      >
        {text(actions.resumes ? t('robot.resume') : t('robot.start'))}
      </CommandButton>
    ),
    onPause === undefined ? null : (
      <CommandButton
        key="pause"
        variant="secondary"
        className={buttonClass}
        disabled={unavailable || !actions.pause}
        busy={pause.busy}
        icon="pause"
        aria-label={t('robot.pauseLabel', { name: label })}
        onClick={() => {
          pause.run()
        }}
      >
        {text(t('robot.pause'))}
      </CommandButton>
    ),
    onReturnToDock === undefined ? null : (
      <CommandButton
        key="dock"
        variant="secondary"
        className={buttonClass}
        disabled={unavailable || !actions.dock}
        busy={dock.busy}
        icon="to_dock"
        aria-label={t('robot.dockLabel', { name: label })}
        onClick={() => {
          dock.run()
        }}
      >
        {text(t('robot.dock'))}
      </CommandButton>
    ),
  ].filter((button) => button !== null)

  // On a small tile the battery leaves the header, so the status has room.
  const battery =
    batteryPercent === undefined ? undefined : (
      <Battery percent={batteryPercent} low={batteryLow} charging={status === 'charging'} />
    )

  return (
    <div ref={ref} className="h-full min-w-0" data-slot="robot-vacuum" data-status={status}>
      <WidgetFrame
        icon={icon}
        title={label}
        subtitle={key === null ? undefined : t(key)}
        active={active}
        pending={pending}
        unavailable={unavailable}
        headerAction={compact ? undefined : battery}
      >
        {working && !unavailable && (
          <span aria-hidden className="relative block h-1.5 w-full overflow-hidden rounded-full bg-primary/15">
            <span className="robot-sweep absolute inset-y-0 left-0 w-1/3 rounded-full bg-primary/70" />
          </span>
        )}
        {compact && battery !== undefined && <div className="flex">{battery}</div>}
        {facts.length > 0 && (
          <p className="text-sm text-muted-foreground tabular-nums">
            <span className="font-medium text-foreground">
              {working ? t('robot.thisClean') : t('robot.lastClean')}
            </span>{' '}
            {facts.join(' · ')}
          </p>
        )}
        {buttons.length > 0 && <div className="flex gap-2">{buttons}</div>}
      </WidgetFrame>
    </div>
  )
}
