/**
 * Pure helpers for the EV charger widget: how power flows between the
 * grid, the sun, a home battery, the house and the car, how close the
 * home is to its contracted power, and how to say it all in plain words.
 * No React, so it is unit tested.
 */
import type { EvChargerStatus, EvChargerWaitReason } from '@/api/generated'

/** Power readings around the charger, in W (signs as in the capability model). */
export interface EnergyReadings {
  /** Positive = taking from the grid, negative = sending to it. */
  readonly gridW?: number
  /** Production, positive. */
  readonly solarW?: number
  /** Positive = the battery supplies the house, negative = it is charging. */
  readonly batteryW?: number
  /** The whole home's consumption, the car included. */
  readonly houseW?: number
  /** Power going to the car. */
  readonly carW?: number
  /** The contracted power (most the home may take from the grid). */
  readonly limitW?: number
}

export type FlowNode = 'grid' | 'solar' | 'battery' | 'car'

/** One line of the diagram, between `node` and the house. */
export interface FlowLink {
  readonly node: FlowNode
  /** Power on this line, positive, in W. */
  readonly watts: number
  /** Beads move: power flows on this line right now. */
  readonly active: boolean
  /** `in`: towards the house (grid import, solar, battery discharge). `out`: away (export, charging). */
  readonly direction: 'in' | 'out'
  /** 0 (slowest) … 5 (fastest). Quantized so small changes never restart the animation. */
  readonly speed: number
  /** Line thickness in diagram units. */
  readonly width: number
}

/** Below this the line is drawn but nothing moves (meter noise). */
export const FLOW_THRESHOLD_W = 40
const SPEED_STEPS_W = [600, 1_500, 3_000, 4_500, 6_500] as const
/** Seconds per bead spacing, by speed step: faster for more power. */
export const FLOW_DURATIONS_S = [2.4, 1.8, 1.35, 1.05, 0.82, 0.64] as const

export function speedStep(watts: number): number {
  const abs = Math.abs(watts)
  let step = 0
  for (const threshold of SPEED_STEPS_W) if (abs >= threshold) step += 1
  return step
}

export function flowDuration(speed: number): string {
  const index = Math.max(0, Math.min(FLOW_DURATIONS_S.length - 1, Math.round(speed)))
  return `${String(FLOW_DURATIONS_S[index])}s`
}

/** 2.5 units for a trickle, up to 6 for a big flow. */
export function flowWidth(watts: number): number {
  const kw = Math.abs(watts) / 1_000
  return Math.round((2.5 + Math.min(3.5, kw * 0.6)) * 10) / 10
}

function link(node: FlowNode, signed: number, inward: boolean): FlowLink {
  const watts = Math.abs(signed)
  return {
    node,
    watts,
    active: watts >= FLOW_THRESHOLD_W,
    direction: inward ? 'in' : 'out',
    speed: speedStep(watts),
    width: flowWidth(watts),
  }
}

/**
 * The lines to draw, for the readings that exist. A missing reading means
 * that node is not in the home (or not known): no line for it.
 */
export function flowLinks(readings: EnergyReadings): FlowLink[] {
  const links: FlowLink[] = []
  const { gridW, solarW, batteryW, carW } = readings
  if (solarW !== undefined) links.push(link('solar', Math.max(0, solarW), true))
  if (gridW !== undefined) links.push(link('grid', gridW, gridW >= 0))
  if (batteryW !== undefined) links.push(link('battery', batteryW, batteryW >= 0))
  if (carW !== undefined) links.push(link('car', Math.max(0, carW), false))
  return links
}

/**
 * The house's consumption: the house meter when there is one, else what
 * comes in from the grid, the sun and the battery.
 */
export function houseConsumption(readings: EnergyReadings): number | undefined {
  if (readings.houseW !== undefined) return Math.max(0, readings.houseW)
  if (readings.gridW === undefined) return undefined
  return Math.max(0, readings.gridW + (readings.solarW ?? 0) + (readings.batteryW ?? 0))
}

/** From this share of the contracted power on, the widget mentions it (gently). */
export const NEAR_LIMIT = 0.9

/**
 * Where the home is against its contracted power: well within it, close
 * to it (from 90 %), or above it (over 100 %). Informative only: the meter
 * tolerates short peaks, and the charger's own control does the rest.
 */
export type LimitLevel = 'normal' | 'near' | 'over'

export function limitLevel(usedW: number, limitW: number): LimitLevel {
  const share = usedW / limitW
  if (share > 1) return 'over'
  return share >= NEAR_LIMIT ? 'near' : 'normal'
}

export interface Headroom {
  /** Power taken from the grid now, W (never negative). */
  readonly usedW: number
  readonly limitW: number
  /** `usedW / limitW`, 0…1 (capped). */
  readonly ratio: number
  readonly level: LimitLevel
}

/** How much of the contracted power the home uses, when both are known. */
export function headroom(readings: EnergyReadings): Headroom | null {
  const { gridW, limitW } = readings
  if (gridW === undefined || limitW === undefined || !(limitW > 0)) return null
  const usedW = Math.max(0, gridW)
  const ratio = Math.min(1, usedW / limitW)
  return { usedW, limitW, ratio, level: limitLevel(usedW, limitW) }
}

/** Translation key for the note under the contracted power. */
export type LimitNoteKey = 'ev.nearLimit' | 'ev.overLimit' | 'ev.overLimitLowering'

/**
 * What to say about the contracted power, or null when there is nothing to
 * say. Above it, "the charger is lowering the charge" only when that is
 * what happens: the car charges and the charger's own power control sets
 * the current.
 */
export function limitNoteKey(
  level: LimitLevel,
  charger: { readonly status?: EvChargerStatus; readonly autoCurrent?: boolean },
): LimitNoteKey | null {
  switch (level) {
    case 'normal':
      return null
    case 'near':
      return 'ev.nearLimit'
    case 'over':
      return charger.status === 'charging' && charger.autoCurrent === true ? 'ev.overLimitLowering' : 'ev.overLimit'
  }
}

/** Translation key for the status line. */
export type StatusKey =
  | 'ev.status.charging'
  | 'ev.status.connected'
  | 'ev.status.waiting'
  | 'ev.status.waitingSchedule'
  | 'ev.status.waitingSolar'
  | 'ev.status.waitingPower'
  | 'ev.status.paused'
  | 'ev.status.disconnected'
  | 'ev.status.error'
  | 'ev.status.locked'
  | 'ev.status.lockedWithCar'

export function statusKey(
  status: EvChargerStatus | undefined,
  waitingFor: EvChargerWaitReason | undefined,
  locked: boolean | undefined,
): StatusKey | null {
  if (status === undefined) return null
  if (locked === true && status !== 'charging' && status !== 'error') {
    return status === 'disconnected' ? 'ev.status.locked' : 'ev.status.lockedWithCar'
  }
  switch (status) {
    case 'charging':
      return 'ev.status.charging'
    case 'connected':
      return 'ev.status.connected'
    case 'waiting':
      switch (waitingFor) {
        case 'schedule':
          return 'ev.status.waitingSchedule'
        case 'solar':
          return 'ev.status.waitingSolar'
        case 'power':
          return 'ev.status.waitingPower'
        case undefined:
          return 'ev.status.waiting'
      }
      return 'ev.status.waiting'
    case 'paused':
      return 'ev.status.paused'
    case 'disconnected':
      return 'ev.status.disconnected'
    case 'error':
      return 'ev.status.error'
  }
  // A status from a newer home: say nothing rather than something wrong.
  return null
}

/** kW with one decimal in the screen's language ("5.1", "5,1"); "0" for nothing. */
export function formatKw(watts: number, language: string): string {
  const kw = Math.abs(watts) < 50 ? 0 : watts / 1_000
  return new Intl.NumberFormat(language, {
    minimumFractionDigits: kw === 0 ? 0 : 1,
    maximumFractionDigits: 1,
  }).format(kw)
}

/** kWh with one decimal. */
export function formatKwh(wattHours: number, language: string): string {
  return new Intl.NumberFormat(language, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(
    Math.max(0, wattHours) / 1_000,
  )
}

/** Amps: whole numbers stay whole, measured values get one decimal. */
export function formatAmps(amps: number, language: string): string {
  return new Intl.NumberFormat(language, { maximumFractionDigits: 1 }).format(amps)
}

export interface Duration {
  readonly hours: number
  readonly minutes: number
}

export function splitDuration(seconds: number): Duration {
  const total = Math.max(0, Math.floor(seconds / 60))
  return { hours: Math.floor(total / 60), minutes: total % 60 }
}

/**
 * What the car is really drawing: the phases that carry current (a
 * single-phase car on a three-phase charger reads 0 on two of them).
 */
export function carryingPhases(phases: readonly number[] | undefined): number[] {
  if (phases === undefined) return []
  const carrying = phases.filter((amps) => amps >= 0.5)
  return carrying.length > 0 ? carrying : []
}

/**
 * The range a charging-current slider may use: the limits set on the
 * charger when it reports them, within what the hardware can do (same rule
 * as the home's `current_range`).
 */
export function chargerCurrentRange(
  features: { readonly min_current_a: number; readonly max_current_a: number },
  state?: { readonly min_current_a?: number; readonly max_current_a?: number },
): { min: number; max: number } {
  const hwMin = Math.min(features.min_current_a, features.max_current_a)
  const hwMax = Math.max(features.min_current_a, features.max_current_a)
  const within = (value: number | undefined, fallback: number) =>
    value === undefined || !Number.isFinite(value) ? fallback : Math.min(hwMax, Math.max(hwMin, value))
  const min = within(state?.min_current_a, hwMin)
  const max = within(state?.max_current_a, hwMax)
  return { min: Math.min(min, max), max: Math.max(min, max) }
}
