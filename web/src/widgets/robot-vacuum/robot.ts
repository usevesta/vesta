/**
 * Pure helpers for the robot vacuum tile: its status in plain words and
 * which buttons make sense right now. No React, so it is unit tested.
 */
import type { RobotVacuumStatus } from '@/api/generated'

export type RobotStatusKey = `robot.status.${RobotVacuumStatus}`

export function statusKey(status: RobotVacuumStatus | undefined): RobotStatusKey | null {
  return status === undefined ? null : `robot.status.${status}`
}

/** The robot is out doing something (the icon lights up). */
export function isBusy(status: RobotVacuumStatus | undefined): boolean {
  return status === 'cleaning' || status === 'returning'
}

/** On its dock (charging or charged). */
export function isHome(status: RobotVacuumStatus | undefined): boolean {
  return status === 'docked' || status === 'charging'
}

export interface RobotActions {
  /** Start, or carry on after a pause. */
  readonly start: boolean
  readonly pause: boolean
  readonly dock: boolean
  /** `start` carries on where it paused. */
  readonly resumes: boolean
}

/**
 * Which buttons work now. Unknown status: nothing (the robot has not said
 * where it is yet).
 */
export function availableActions(status: RobotVacuumStatus | undefined): RobotActions {
  if (status === undefined) return { start: false, pause: false, dock: false, resumes: false }
  return {
    start: status !== 'cleaning',
    pause: status === 'cleaning' || status === 'returning',
    dock: !isHome(status) && status !== 'returning',
    resumes: status === 'paused',
  }
}

/** Splits seconds into whole minutes and hours for "1 h 5 min". */
export function splitMinutes(seconds: number): { hours: number; minutes: number } {
  const total = Math.max(0, Math.round(seconds / 60))
  return { hours: Math.floor(total / 60), minutes: total % 60 }
}

export const LOW_BATTERY_PERCENT = 20
