import { describe, expect, it } from 'vitest'

import type { ChargingScheduleFeatures, ChargingWindow } from '@/api/generated'
import {
  ageOf,
  daysSummary,
  formatTime,
  isOvernight,
  minutesOf,
  newWindow,
  originKey,
  sameHours,
  scheduleProblem,
  toggleDay,
  uniqueWindows,
  WEEK,
} from '@/widgets/ev-charger/schedule'

const FEATURES: ChargingScheduleFeatures = {
  max_windows: 2,
  weekdays: true,
  overnight: true,
  editable: true,
  reports_windows: false,
  saving_needs_internet: true,
  refreshable: true,
}

const night: ChargingWindow = { start: '22:00', end: '06:00', days: ['monday', 'friday'] }

describe('charging hours', () => {
  it('reads times and knows a window past midnight', () => {
    expect(minutesOf('22:30')).toBe(1_350)
    expect(minutesOf('24:00')).toBeNull()
    expect(minutesOf('7:00')).toBeNull()
    expect(isOvernight(night)).toBe(true)
    expect(isOvernight(newWindow())).toBe(false)
  })

  it('lets Save through only for hours that fit the charger', () => {
    expect(scheduleProblem([night], FEATURES)).toBeNull()
    expect(scheduleProblem([night, newWindow()], FEATURES)).toBeNull()
    expect(scheduleProblem([], FEATURES)).toBe('ev.hours.problem.none')
    expect(scheduleProblem([night, night, night], FEATURES)).toBe('ev.hours.problem.tooMany')
    expect(scheduleProblem([{ ...night, days: [] }], FEATURES)).toBe('ev.hours.problem.noDays')
    expect(scheduleProblem([{ ...night, end: '22:00' }], FEATURES)).toBe('ev.hours.problem.sameTime')
    expect(scheduleProblem([{ ...night, start: '' }], FEATURES)).toBe('ev.hours.problem.time')
    expect(scheduleProblem([night], { ...FEATURES, overnight: false })).toBe('ev.hours.problem.overnight')
  })

  it('says the days in plain words', () => {
    expect(daysSummary(WEEK)).toEqual({ key: 'ev.hours.everyDay' })
    expect(daysSummary(['friday', 'monday', 'tuesday', 'wednesday', 'thursday'])).toEqual({
      key: 'ev.hours.workdays',
    })
    expect(daysSummary(['sunday', 'saturday'])).toEqual({ key: 'ev.hours.weekend' })
    expect(daysSummary(['friday', 'monday'])).toEqual({ key: 'ev.hours.someDays', days: ['monday', 'friday'] })
  })

  it('toggles days and keeps them in week order', () => {
    expect(toggleDay(['friday'], 'monday')).toEqual(['monday', 'friday'])
    expect(toggleDay(['monday', 'friday'], 'monday')).toEqual(['friday'])
  })

  it('writes times the way the language does', () => {
    expect(formatTime('22:00', 'es')).toBe('22:00')
    expect(formatTime('07:05', 'es')).toBe('7:05')
    expect(formatTime('22:00', 'en-US')).toMatch(/^10:00\s?PM$/)
  })

  it('shows the same hours twice only once', () => {
    expect(uniqueWindows([night, { ...night, days: ['friday', 'monday'] }, newWindow()])).toEqual([night, newWindow()])
  })

  it('knows when the hours on screen changed, order and repeats aside', () => {
    const weekend: ChargingWindow = { start: '10:00', end: '14:00', days: ['saturday', 'sunday'] }
    expect(sameHours([night, weekend], [weekend, { ...night, days: ['friday', 'monday'] }, night])).toBe(true)
    expect(sameHours([night], [night, weekend])).toBe(false)
    expect(sameHours([night], [{ ...night, end: '06:30' }])).toBe(false)
    expect(sameHours([], [])).toBe(true)
  })

  it('says where the hours came from', () => {
    const at = '2026-11-28T09:30:00Z'
    expect(originKey({ source: 'charger', at, restored: false })).toBe('ev.hours.origin.charger')
    expect(originKey({ source: 'saved', at, restored: false })).toBe('ev.hours.origin.saved')
    expect(originKey({ source: 'charger', at, restored: true })).toBe('ev.hours.origin.restoredCharger')
    expect(originKey({ source: 'saved', at, restored: true })).toBe('ev.hours.origin.restoredSaved')
  })

  it('says how long ago in plain words', () => {
    const now = new Date('2026-11-28T12:00:00Z')
    const ago = (at: string, language = 'en') => ageOf(at, now, language)
    expect(ago('2026-11-28T11:59:30Z')).toEqual({ key: 'ev.hours.justNow' })
    // A clock a little ahead is still "just now".
    expect(ago('2026-11-28T12:00:20Z')).toEqual({ key: 'ev.hours.justNow' })
    expect(ago('2026-11-28T11:55:00Z')).toEqual({ text: '5 minutes ago' })
    expect(ago('2026-11-28T11:55:00Z', 'es')).toEqual({ text: 'hace 5 minutos' })
    expect(ago('2026-11-28T09:00:00Z')).toEqual({ text: '3 hours ago' })
    expect(ago('2026-11-27T10:00:00Z')).toEqual({ text: 'yesterday' })
    expect(ago('2026-11-27T10:00:00Z', 'es')).toEqual({ text: 'ayer' })
    expect(ago('2026-11-20T12:00:00Z', 'es')).toEqual({ text: 'hace 8 días' })
    expect(ago('not a time')).toEqual({ text: '' })
  })
})
