/**
 * The sample home's plugs and robot vacuum (demo mode only): a lamp plug
 * that can be switched, a washing machine plug that is only watched, and a
 * robot on a compressed clock (a tick of 2 s is a minute of cleaning). A
 * small cousin of `appliance.rs` in `vesta-driver-sim`; not a model of any
 * real protocol.
 */
import { create } from 'zustand'

import type { RobotVacuumStatus } from '@/api/generated'

export const DEMO_APPLIANCE_TICK_MS = 2_000
const LATENCY_MS = 600
const VOLTS = 230
const FULL_CLEAN_M2 = 40
const RETURN_AT_PERCENT = 15

export type DemoPlugId = 'hallLamp' | 'washer'

export interface DemoPlug {
  readonly on: boolean
  readonly powerW: number
  readonly energyWh: number
  /** Only watched: no switch. */
  readonly watchOnly: boolean
  readonly pending: boolean
}

export interface DemoRobot {
  readonly status: RobotVacuumStatus
  readonly batteryPercent: number
  readonly cleanedAreaM2: number
  readonly cleaningDurationS: number
  readonly pending: boolean
}

export interface DemoAppliances {
  readonly plugs: Readonly<Record<DemoPlugId, DemoPlug>>
  readonly robot: DemoRobot
}

const LOADS_W: Readonly<Record<DemoPlugId, number>> = { hallLamp: 42, washer: 480 }

export const useDemoAppliances = create<DemoAppliances>()(() => ({
  plugs: {
    hallLamp: { on: true, powerW: LOADS_W.hallLamp, energyWh: 18_400, watchOnly: false, pending: false },
    washer: { on: true, powerW: LOADS_W.washer, energyWh: 212_000, watchOnly: true, pending: false },
  },
  robot: { status: 'docked', batteryPercent: 100, cleanedAreaM2: 22, cleaningDurationS: 1_800, pending: false },
}))

export function plugCurrentA(plug: DemoPlug): number {
  return Math.round((plug.powerW / VOLTS) * 1_000) / 1_000
}

export const DEMO_VOLTS = VOLTS

let returning = 0
let tickCount = 0

function nextRobot(robot: DemoRobot): DemoRobot {
  switch (robot.status) {
    case 'cleaning': {
      const batteryPercent = Math.max(0, robot.batteryPercent - 1)
      const cleanedAreaM2 = robot.cleanedAreaM2 + 0.5
      const done = cleanedAreaM2 >= FULL_CLEAN_M2 || batteryPercent <= RETURN_AT_PERCENT
      if (done) returning = 3
      return {
        ...robot,
        batteryPercent,
        cleanedAreaM2,
        cleaningDurationS: robot.cleaningDurationS + 60,
        status: done ? 'returning' : 'cleaning',
      }
    }
    case 'returning':
      returning -= 1
      return returning <= 0 ? { ...robot, status: 'charging' } : robot
    case 'charging': {
      const batteryPercent = Math.min(100, robot.batteryPercent + 2)
      return { ...robot, batteryPercent, status: batteryPercent >= 100 ? 'docked' : 'charging' }
    }
    case 'docked':
    case 'paused':
    case 'idle':
    case 'error':
      return robot
  }
}

function step(state: DemoAppliances): DemoAppliances {
  tickCount += 1
  const plugs = { ...state.plugs }
  for (const id of Object.keys(plugs) as DemoPlugId[]) {
    const plug = plugs[id]
    const wobble = 1 + 0.03 * Math.sin(tickCount * (id === 'washer' ? 1.7 : 0.9))
    const powerW = plug.on ? Math.round(LOADS_W[id] * wobble * 10) / 10 : 0
    plugs[id] = { ...plug, powerW, energyWh: plug.energyWh + (powerW * DEMO_APPLIANCE_TICK_MS) / 3_600_000 }
  }
  return { plugs, robot: nextRobot(state.robot) }
}

/** Runs the sample plugs and robot while the demo is on screen. Returns a stop function. */
export function startDemoAppliances(): () => void {
  const timer = setInterval(() => {
    useDemoAppliances.setState(step(useDemoAppliances.getState()))
  }, DEMO_APPLIANCE_TICK_MS)
  return () => {
    clearInterval(timer)
  }
}

const wait = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })

/** Switches a plug the way a real one would: after a moment. */
export async function switchDemoPlug(id: DemoPlugId, on: boolean): Promise<boolean> {
  const state = useDemoAppliances.getState()
  const plug = state.plugs[id]
  if (plug.watchOnly) return false
  useDemoAppliances.setState({ plugs: { ...state.plugs, [id]: { ...plug, on, pending: true } } })
  await wait(LATENCY_MS)
  const now = useDemoAppliances.getState()
  const current = now.plugs[id]
  useDemoAppliances.setState({
    plugs: { ...now.plugs, [id]: { ...current, powerW: on ? LOADS_W[id] : 0, pending: false } },
  })
  return true
}

export type DemoRobotCommand = 'start' | 'pause' | 'dock'

/** Sends the sample robot a command; it answers after a moment. */
export async function commandDemoRobot(command: DemoRobotCommand): Promise<boolean> {
  useDemoAppliances.setState((state) => ({ robot: { ...state.robot, pending: true } }))
  await wait(LATENCY_MS)
  const { robot } = useDemoAppliances.getState()
  let next: DemoRobot = { ...robot, pending: false }
  switch (command) {
    case 'start':
      if (robot.status === 'paused') next = { ...next, status: 'cleaning' }
      else if (robot.status !== 'cleaning') {
        next = { ...next, status: 'cleaning', cleanedAreaM2: 0, cleaningDurationS: 0 }
      }
      break
    case 'pause':
      if (robot.status === 'cleaning' || robot.status === 'returning') next = { ...next, status: 'paused' }
      break
    case 'dock':
      if (robot.status !== 'docked' && robot.status !== 'charging') {
        returning = 3
        next = { ...next, status: 'returning' }
      }
      break
  }
  useDemoAppliances.setState({ robot: next })
  return true
}
