/**
 * The sample home's car charger (demo mode only): a car plugged in, solar
 * panels, a home battery and the house, on a compressed day (12 minutes
 * for 24 hours) that starts mid-morning. Like a real charger, it keeps the
 * grid under the contracted power by itself, lowering the current or
 * holding the car back. A small cousin of the simulator in
 * `vesta-driver-sim` (`ev.rs`); not a model of any real protocol.
 */
import { create } from 'zustand'

import type {
  ChargingSchedule,
  ChargingScheduleFeatures,
  ChargingWindow,
  EvChargeMode,
  EvChargerStatus,
  EvChargerWaitReason,
} from '@/api/generated'
import type { EnergyReadings } from '@/widgets/ev-charger/energy'
import { minutesOf, WEEK } from '@/widgets/ev-charger/schedule'

export const DEMO_CHARGER_TICK_MS = 2_000
const LATENCY_MS = 600
const DAY_SECONDS = 720
const START_HOUR = 10.5
const CONTRACTED_W = 5_750
const VOLTS = 230
export const DEMO_CHARGER_MODES: readonly EvChargeMode[] = ['manual', 'solar', 'solar_grid']
export const DEMO_CURRENT_RANGE = { min: 6, max: 25 } as const
const BATTERY_WH = 10_000
const BATTERY_MAX_W = 2_500
/** Saving hours goes "through the internet": slower than other settings. */
const SAVE_HOURS_MS = 1_600
/** Reading them too. */
const READ_HOURS_MS = 1_200
/** The sample home last read its charger's hours a while ago. */
const LAST_READ_MS = 3 * 60 * 60 * 1_000

/**
 * Like a charger whose hours are not reported: saved and read (when the
 * hours editor opens) through the internet.
 */
export const DEMO_SCHEDULE_FEATURES: ChargingScheduleFeatures = {
  max_windows: 2,
  weekdays: true,
  overnight: true,
  editable: true,
  reports_windows: false,
  saving_needs_internet: true,
  refreshable: true,
}

export interface DemoCharger {
  readonly status: EvChargerStatus
  readonly waitingFor?: EvChargerWaitReason
  readonly powerW: number
  readonly sessionEnergyWh: number
  readonly sessionDurationS: number
  readonly currentA: number
  readonly measuredA: readonly number[]
  readonly mode: EvChargeMode
  readonly locked: boolean
  readonly paused: boolean
  readonly scheduleEnabled: boolean
  /** The hours it follows (23:00–07:00 every day until changed). */
  readonly windows: readonly ChargingWindow[]
  /** The hours as last read or saved, with where from and when. */
  readonly chargingSchedule?: ChargingSchedule
  readonly readings: EnergyReadings
  readonly pending: boolean
}

interface Sim {
  elapsed: number
  offeredA: number
  batteryWh: number
}

const sim: Sim = { elapsed: 0, offeredA: 16, batteryWh: BATTERY_WH * 0.55 }

function hourOf(elapsed: number): number {
  return (((START_HOUR + (elapsed / DAY_SECONDS) * 24) % 24) + 24) % 24
}

function solarAt(hour: number, elapsed: number): number {
  if (hour < 7.5 || hour >= 20.5) return 0
  const x = (hour - 7.5) / 13
  const clouds = 1 - 0.45 * Math.max(0, Math.sin(elapsed / 37) * Math.sin(elapsed / 11))
  return 4_200 * Math.sin(Math.PI * x) ** 1.4 * clouds
}

function houseAt(hour: number, elapsed: number): number {
  const on = (from: number, to: number) => hour >= from && hour < to
  let load = 380 + 25 * Math.sin(elapsed / 3)
  if (on(10.9, 11.9)) load += 1_900 // washing machine
  if (on(13.2, 13.9) || on(20.6, 21.3)) load += 2_100 // oven
  if (on(18.5, 22.5)) load += 450
  if (elapsed % 100 < 40) load += 110
  return load
}

type Settings = Pick<DemoCharger, 'mode' | 'locked' | 'paused' | 'scheduleEnabled' | 'currentA' | 'windows'>

/** Day of the week (the demo starts on a Monday) and minute of the day. */
function dayAndMinute(elapsed: number): [number, number] {
  const hours = START_HOUR + (elapsed / DAY_SECONDS) * 24
  return [Math.floor(hours / 24) % 7, Math.floor((hours % 24) * 60)]
}

/** Within the charging hours; a window past midnight belongs to the day it starts. */
function withinHours(windows: readonly ChargingWindow[], elapsed: number): boolean {
  const [day, minute] = dayAndMinute(elapsed)
  const on = (window: ChargingWindow, index: number) => window.days.includes(WEEK[index] ?? 'monday')
  return windows.some((window) => {
    const start = minutesOf(window.start) ?? 0
    const end = minutesOf(window.end) ?? 0
    if (start < end) return on(window, day) && minute >= start && minute < end
    return (on(window, day) && minute >= start) || (on(window, (day + 6) % 7) && minute < end)
  })
}

/** Everything the charger and the meters show after `dt` seconds. */
function step(previous: DemoCharger, settings: Settings, dt: number): DemoCharger {
  sim.elapsed += dt
  const hour = hourOf(sim.elapsed)
  const solar = solarAt(hour, sim.elapsed)
  const base = houseAt(hour, sim.elapsed)
  let battery = 0
  const net = solar - base
  if (net > 0 && sim.batteryWh < BATTERY_WH) battery = -Math.min(net, BATTERY_MAX_W)
  else if (net < 0 && sim.batteryWh > 1_000) battery = Math.min(-net, BATTERY_MAX_W)
  sim.batteryWh = Math.min(BATTERY_WH, Math.max(0, sim.batteryWh - (battery * dt) / 3_600))

  const gridWithoutCar = base - solar - battery
  const allowedA = Math.min(DEMO_CURRENT_RANGE.max, Math.floor((CONTRACTED_W - gridWithoutCar - 60) / VOLTS))
  const surplusA = Math.max(0, -gridWithoutCar) / VOLTS
  const wantedA =
    settings.mode === 'solar'
      ? Math.min(Math.floor(surplusA), settings.currentA)
      : settings.mode === 'solar_grid'
        ? Math.min(Math.max(Math.floor(surplusA), DEMO_CURRENT_RANGE.min), settings.currentA)
        : settings.currentA
  const inSchedule = withinHours(settings.windows, sim.elapsed)

  let status: EvChargerStatus = 'charging'
  let waitingFor: EvChargerWaitReason | undefined
  if (settings.paused) status = 'paused'
  else if (settings.locked) status = 'connected'
  else if (settings.scheduleEnabled && !inSchedule) [status, waitingFor] = ['waiting', 'schedule']
  else if (settings.mode === 'solar' && wantedA < DEMO_CURRENT_RANGE.min) [status, waitingFor] = ['waiting', 'solar']
  else if (allowedA < DEMO_CURRENT_RANGE.min) [status, waitingFor] = ['waiting', 'power']

  if (status === 'charging') {
    const target = Math.max(DEMO_CURRENT_RANGE.min, Math.min(wantedA, allowedA))
    sim.offeredA = target > sim.offeredA ? Math.min(target, Math.max(sim.offeredA, DEMO_CURRENT_RANGE.min) + 4) : target
  } else {
    sim.offeredA = 0
  }
  const carW = status === 'charging' ? sim.offeredA * VOLTS : 0
  const house = base + carW
  return {
    ...previous,
    ...settings,
    status,
    waitingFor,
    powerW: Math.round(carW),
    sessionEnergyWh: previous.sessionEnergyWh + (carW * dt) / 3_600,
    sessionDurationS: previous.sessionDurationS + dt,
    measuredA: [status === 'charging' ? Math.round(sim.offeredA * 10) / 10 : 0],
    readings: {
      gridW: Math.round(house - solar - battery),
      limitW: CONTRACTED_W,
      houseW: Math.round(house),
      solarW: Math.round(solar),
      batteryW: Math.round(battery),
    },
  }
}

const INITIAL_SETTINGS: Settings = {
  mode: 'manual',
  locked: false,
  paused: false,
  scheduleEnabled: false,
  currentA: 16,
  windows: [{ start: '23:00', end: '07:00', days: [...WEEK] }],
}

export const useDemoCharger = create<DemoCharger>()(() =>
  step(
    {
      ...INITIAL_SETTINGS,
      status: 'charging',
      powerW: 0,
      sessionEnergyWh: 4_850,
      sessionDurationS: 1_920,
      measuredA: [],
      readings: {},
      // Kept from an earlier visit: the last known hours.
      chargingSchedule: {
        enabled: INITIAL_SETTINGS.scheduleEnabled,
        windows: [...INITIAL_SETTINGS.windows],
        origin: { source: 'charger', at: new Date(Date.now() - LAST_READ_MS).toISOString(), restored: true },
      },
      pending: false,
    },
    INITIAL_SETTINGS,
    0,
  ),
)

function settingsOf(state: DemoCharger): Settings {
  const { mode, locked, paused, scheduleEnabled, currentA, windows } = state
  return { mode, locked, paused, scheduleEnabled, currentA, windows }
}

/** Runs the sample charger while the demo is on screen. Returns a stop function. */
export function startDemoCharger(): () => void {
  const timer = setInterval(() => {
    const state = useDemoCharger.getState()
    useDemoCharger.setState(step(state, settingsOf(state), DEMO_CHARGER_TICK_MS / 1_000))
  }, DEMO_CHARGER_TICK_MS)
  return () => {
    clearInterval(timer)
  }
}

let latest = 0

/** Changes a setting the way a charger would: after a moment, then it shows. */
export async function changeDemoCharger(patch: Partial<Settings>): Promise<boolean> {
  const sequence = ++latest
  useDemoCharger.setState({ ...patch, pending: true })
  await new Promise<void>((resolve) => {
    setTimeout(resolve, LATENCY_MS)
  })
  const state = useDemoCharger.getState()
  const settings = { ...settingsOf(state), ...patch }
  if (settings.currentA !== state.currentA) {
    settings.currentA = Math.min(DEMO_CURRENT_RANGE.max, Math.max(DEMO_CURRENT_RANGE.min, Math.round(settings.currentA)))
  }
  const next = step(state, settings, 0)
  const known = state.chargingSchedule
  useDemoCharger.setState({
    ...next,
    // The hours shown follow the on/off switch once they are known.
    chargingSchedule: known === undefined ? undefined : { ...known, enabled: next.scheduleEnabled },
    pending: sequence !== latest,
  })
  return true
}

/** Saves charging hours "through the internet": a little slower, then they show. */
export async function saveDemoSchedule(schedule: ChargingSchedule): Promise<boolean> {
  const sequence = ++latest
  useDemoCharger.setState({ pending: true })
  await new Promise<void>((resolve) => {
    setTimeout(resolve, SAVE_HOURS_MS)
  })
  const state = useDemoCharger.getState()
  const settings: Settings = { ...settingsOf(state), scheduleEnabled: schedule.enabled, windows: schedule.windows }
  useDemoCharger.setState({
    ...step(state, settings, 0),
    chargingSchedule: {
      enabled: schedule.enabled,
      windows: schedule.windows,
      origin: { source: 'saved', at: new Date().toISOString(), restored: false },
    },
    pending: sequence !== latest,
  })
  return true
}

/** Reads the hours set on the charger "through the internet": a moment, then they show. */
export async function refreshDemoSchedule(): Promise<boolean> {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, READ_HOURS_MS)
  })
  const state = useDemoCharger.getState()
  useDemoCharger.setState({
    chargingSchedule: {
      enabled: state.scheduleEnabled,
      windows: [...state.windows],
      origin: { source: 'charger', at: new Date().toISOString(), restored: false },
    },
  })
  return true
}
