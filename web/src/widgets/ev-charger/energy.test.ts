import { describe, expect, it } from 'vitest'

import {
  carryingPhases,
  chargerCurrentRange,
  flowDuration,
  flowLinks,
  flowWidth,
  formatKw,
  formatKwh,
  headroom,
  houseConsumption,
  speedStep,
  splitDuration,
  statusKey,
} from '@/widgets/ev-charger/energy'

describe('energy flow', () => {
  it('draws a line for every reading the home has, in a fixed order', () => {
    const links = flowLinks({ gridW: 5_300, solarW: 800, houseW: 6_100, carW: 5_092 })
    expect(links.map((link) => link.node)).toEqual(['solar', 'grid', 'car'])
    expect(flowLinks({ houseW: 400 })).toEqual([])
  })

  it('takes direction from the sign of each reading', () => {
    const byNode = (readings: Parameters<typeof flowLinks>[0]) =>
      Object.fromEntries(flowLinks(readings).map((link) => [link.node, link.direction]))
    // Importing from the grid, sun producing, battery supplying, car charging.
    expect(byNode({ gridW: 1_200, solarW: 900, batteryW: 400, carW: 2_000 })).toEqual({
      grid: 'in',
      solar: 'in',
      battery: 'in',
      car: 'out',
    })
    // Exporting to the grid, the battery charging.
    expect(byNode({ gridW: -1_500, batteryW: -700 })).toEqual({ grid: 'out', battery: 'out' })
  })

  it('only moves beads for real power, faster and thicker for more', () => {
    const [idle] = flowLinks({ gridW: 25 })
    expect(idle?.active).toBe(false)
    const [trickle] = flowLinks({ gridW: 300 })
    const [big] = flowLinks({ gridW: -7_000 })
    expect(trickle?.active).toBe(true)
    expect(big?.watts).toBe(7_000)
    expect(big?.speed).toBeGreaterThan(trickle?.speed ?? 99)
    expect(big?.width).toBeGreaterThan(trickle?.width ?? 99)
    // Night solar reads slightly negative on some inverters: never a flow.
    expect(flowLinks({ solarW: -12 })[0]?.watts).toBe(0)
  })

  it('quantizes speed so small changes never restart the animation', () => {
    expect(speedStep(0)).toBe(0)
    expect(speedStep(599)).toBe(0)
    expect(speedStep(600)).toBe(1)
    expect(speedStep(-5_000)).toBe(4)
    expect(speedStep(20_000)).toBe(5)
    expect(speedStep(4_510)).toBe(speedStep(4_990))
    expect(flowDuration(0)).toBe('2.4s')
    expect(flowDuration(5)).toBe('0.64s')
    expect(flowDuration(42)).toBe('0.64s')
    expect(flowWidth(0)).toBe(2.5)
    expect(flowWidth(20_000)).toBe(6)
  })

  it('uses the house meter, or works the house out from its sources', () => {
    expect(houseConsumption({ houseW: 6_100, gridW: 5_300 })).toBe(6_100)
    expect(houseConsumption({ gridW: 1_000, solarW: 2_000, batteryW: -500 })).toBe(2_500)
    expect(houseConsumption({ gridW: -3_000, solarW: 2_000 })).toBe(0)
    expect(houseConsumption({ solarW: 2_000 })).toBeUndefined()
  })
})

describe('contracted power', () => {
  it('compares what comes from the grid with the contracted power', () => {
    expect(headroom({ gridW: 2_875, limitW: 5_750 })).toEqual({
      usedW: 2_875,
      limitW: 5_750,
      ratio: 0.5,
      nearLimit: false,
    })
    expect(headroom({ gridW: 5_300, limitW: 5_750 })?.nearLimit).toBe(true)
    expect(headroom({ gridW: 6_000, limitW: 5_750 })?.ratio).toBe(1)
    // Sending power to the grid uses none of it.
    expect(headroom({ gridW: -800, limitW: 5_750 })?.usedW).toBe(0)
    expect(headroom({ gridW: 1_000 })).toBeNull()
    expect(headroom({ gridW: 1_000, limitW: 0 })).toBeNull()
  })
})

describe('status in plain words', () => {
  it('picks the sentence for each situation', () => {
    expect(statusKey(undefined, undefined, undefined)).toBeNull()
    expect(statusKey('charging', undefined, false)).toBe('ev.status.charging')
    expect(statusKey('connected', undefined, undefined)).toBe('ev.status.connected')
    expect(statusKey('waiting', 'schedule', false)).toBe('ev.status.waitingSchedule')
    expect(statusKey('waiting', 'solar', false)).toBe('ev.status.waitingSolar')
    expect(statusKey('waiting', 'power', false)).toBe('ev.status.waitingPower')
    expect(statusKey('waiting', undefined, false)).toBe('ev.status.waiting')
    expect(statusKey('paused', undefined, false)).toBe('ev.status.paused')
    expect(statusKey('disconnected', undefined, false)).toBe('ev.status.disconnected')
    expect(statusKey('error', undefined, true)).toBe('ev.status.error')
  })

  it('says when it is locked', () => {
    expect(statusKey('disconnected', undefined, true)).toBe('ev.status.locked')
    expect(statusKey('connected', undefined, true)).toBe('ev.status.lockedWithCar')
    expect(statusKey('paused', undefined, true)).toBe('ev.status.lockedWithCar')
  })
})

describe('numbers', () => {
  it('formats power and energy in the screen language', () => {
    expect(formatKw(5_092, 'en')).toBe('5.1')
    expect(formatKw(5_092, 'es')).toBe('5,1')
    expect(formatKw(30, 'en')).toBe('0')
    expect(formatKw(-1_250, 'en')).toBe('-1.3')
    expect(formatKwh(4_890, 'es')).toBe('4,9')
    expect(formatKwh(-3, 'en')).toBe('0.0')
  })

  it('splits durations and finds the phases that carry current', () => {
    expect(splitDuration(3_841)).toEqual({ hours: 1, minutes: 4 })
    expect(splitDuration(59)).toEqual({ hours: 0, minutes: 0 })
    expect(splitDuration(-5)).toEqual({ hours: 0, minutes: 0 })
    expect(carryingPhases([22.94, 0, 0])).toEqual([22.94])
    expect(carryingPhases([16, 15.9, 16.1])).toEqual([16, 15.9, 16.1])
    expect(carryingPhases([0.1, 0, 0])).toEqual([])
    expect(carryingPhases(undefined)).toEqual([])
  })

  it('keeps the current slider within the charger and the hardware', () => {
    const features = { min_current_a: 6, max_current_a: 32 }
    expect(chargerCurrentRange(features)).toEqual({ min: 6, max: 32 })
    expect(chargerCurrentRange(features, { min_current_a: 8, max_current_a: 25 })).toEqual({ min: 8, max: 25 })
    expect(chargerCurrentRange(features, { min_current_a: 2, max_current_a: 40 })).toEqual({ min: 6, max: 32 })
    expect(chargerCurrentRange(features, { min_current_a: 20, max_current_a: 10 })).toEqual({ min: 10, max: 20 })
    expect(chargerCurrentRange({ min_current_a: 32, max_current_a: 6 })).toEqual({ min: 6, max: 32 })
  })
})
