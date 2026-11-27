/**
 * Local mock state for demo mode (`?demo=1`). Simulates device latency so
 * pending states are visible, one offline device, and one device that never
 * confirms (to show the revert-and-error path). Demo only: not a model of
 * the real protocol.
 */
import { create } from 'zustand'

import type { CoverMotion } from '@/widgets/cover-control/CoverControl'
import type { FanDirection } from '@/widgets/fan-control/FanControl'
import type { LightColorTemperature } from '@/widgets/light-control/LightControl'

export const DEMO_LATENCY_MS = 600
export const DEMO_TIMEOUT_MS = 2_500
const COVER_TICK_MS = 150
const COVER_STEP = 4
const GARAGE_TRAVEL_MS = 3_000
const DOOR_PULSE_MS = 1_500

export interface DemoLight {
  on: boolean
  brightness?: number
  colorTemperature?: LightColorTemperature
  pending: boolean
  unavailable: boolean
  /** Never confirms: commands time out and revert. */
  failing: boolean
}

export interface DemoFan {
  on: boolean
  speed: number
  speedSteps: number
  direction?: FanDirection
  pending: boolean
}

export interface DemoCover {
  position?: number
  isOpen?: boolean
  motion: CoverMotion
  pending: boolean
}

export interface DemoDoor {
  pending: boolean
}

export type DemoLightId = 'livingRoom' | 'fanLight' | 'kitchen' | 'hallway' | 'porch'
export type DemoFanId = 'ceiling'
export type DemoCoverId = 'bedroomBlind' | 'patioAwning' | 'garage'
export type DemoDoorId = 'front'
export type CoverTarget = 'open' | 'close' | 'stop' | number

type LightPatch = Partial<Pick<DemoLight, 'on' | 'brightness' | 'colorTemperature'>>
type FanPatch = Partial<Pick<DemoFan, 'on' | 'speed' | 'direction'>>

export interface DemoState {
  lights: Record<DemoLightId, DemoLight>
  fans: Record<DemoFanId, DemoFan>
  covers: Record<DemoCoverId, DemoCover>
  doors: Record<DemoDoorId, DemoDoor>
  /** Resolves to false when the device did not confirm (state reverted). */
  setLight: (id: DemoLightId, patch: LightPatch) => Promise<boolean>
  setFan: (id: DemoFanId, patch: FanPatch) => Promise<boolean>
  moveCover: (id: DemoCoverId, target: CoverTarget) => void
  openDoor: (id: DemoDoorId) => Promise<boolean>
}

const wait = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })

/** Latest command per device: only the newest one may clear `pending`. */
const sequence = new Map<string, number>()
function nextSequence(key: string): number {
  const value = (sequence.get(key) ?? 0) + 1
  sequence.set(key, value)
  return value
}
const isLatest = (key: string, value: number) => sequence.get(key) === value

const coverTimers = new Map<DemoCoverId, () => void>()
function stopCoverTimer(id: DemoCoverId) {
  coverTimers.get(id)?.()
  coverTimers.delete(id)
}

export const useDemoStore = create<DemoState>()((set, get) => {
  const patchLight = (id: DemoLightId, patch: Partial<DemoLight>) => {
    set((s) => ({ lights: { ...s.lights, [id]: { ...s.lights[id], ...patch } } }))
  }
  const patchFan = (id: DemoFanId, patch: Partial<DemoFan>) => {
    set((s) => ({ fans: { ...s.fans, [id]: { ...s.fans[id], ...patch } } }))
  }
  const patchCover = (id: DemoCoverId, patch: Partial<DemoCover>) => {
    set((s) => ({ covers: { ...s.covers, [id]: { ...s.covers[id], ...patch } } }))
  }

  return {
    lights: {
      livingRoom: {
        on: true,
        brightness: 80,
        colorTemperature: { kelvin: 3_000, minKelvin: 2_700, maxKelvin: 6_500 },
        pending: false,
        unavailable: false,
        failing: false,
      },
      fanLight: { on: false, brightness: 60, pending: false, unavailable: false, failing: false },
      kitchen: { on: true, pending: false, unavailable: false, failing: false },
      hallway: { on: false, brightness: 40, pending: false, unavailable: true, failing: false },
      porch: { on: false, pending: false, unavailable: false, failing: true },
    },
    fans: {
      ceiling: { on: true, speed: 3, speedSteps: 6, direction: 'forward', pending: false },
    },
    covers: {
      bedroomBlind: { position: 40, motion: 'stopped', pending: false },
      patioAwning: { position: 0, motion: 'stopped', pending: false },
      garage: { isOpen: false, motion: 'stopped', pending: false },
    },
    doors: {
      front: { pending: false },
    },

    async setLight(id, patch) {
      const before = get().lights[id]
      if (before.unavailable) return false
      const key = `light:${id}`
      const seq = nextSequence(key)
      patchLight(id, { ...patch, pending: true })
      if (before.failing) {
        await wait(DEMO_TIMEOUT_MS)
        if (isLatest(key, seq)) patchLight(id, { ...before, pending: false })
        return false
      }
      await wait(DEMO_LATENCY_MS)
      if (isLatest(key, seq)) patchLight(id, { pending: false })
      return true
    },

    async setFan(id, patch) {
      const key = `fan:${id}`
      const seq = nextSequence(key)
      patchFan(id, { ...patch, pending: true })
      await wait(DEMO_LATENCY_MS)
      if (isLatest(key, seq)) patchFan(id, { pending: false })
      return true
    },

    moveCover(id, target) {
      stopCoverTimer(id)
      const cover = get().covers[id]
      if (target === 'stop') {
        patchCover(id, { motion: 'stopped', pending: false })
        return
      }
      const goal = target === 'open' ? 100 : target === 'close' ? 0 : Math.round(target)

      if (cover.position === undefined) {
        // No position reported (e.g. garage door): travel, then end-stop.
        patchCover(id, { motion: goal > 0 ? 'opening' : 'closing', pending: true })
        const timer = setTimeout(() => {
          coverTimers.delete(id)
          patchCover(id, { motion: 'stopped', isOpen: goal > 0, pending: false })
        }, GARAGE_TRAVEL_MS)
        coverTimers.set(id, () => {
          clearTimeout(timer)
        })
        return
      }

      if (goal === cover.position) return
      patchCover(id, { motion: goal > cover.position ? 'opening' : 'closing', pending: true })
      const interval = setInterval(() => {
        const current = get().covers[id].position ?? goal
        const step = Math.sign(goal - current) * Math.min(COVER_STEP, Math.abs(goal - current))
        const next = current + step
        if (next === goal) {
          stopCoverTimer(id)
          patchCover(id, { position: next, motion: 'stopped', pending: false })
        } else {
          patchCover(id, { position: next, pending: false })
        }
      }, COVER_TICK_MS)
      coverTimers.set(id, () => {
        clearInterval(interval)
      })
    },

    async openDoor(id) {
      set((s) => ({ doors: { ...s.doors, [id]: { pending: true } } }))
      await wait(DOOR_PULSE_MS)
      set((s) => ({ doors: { ...s.doors, [id]: { pending: false } } }))
      return true
    },
  }
})
