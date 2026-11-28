import { describe, expect, it } from 'vitest'

import type { ChargingScheduleFeatures, ChargingWindow } from '@/api/generated'
import {
  daysSummary,
  formatTime,
  isOvernight,
  minutesOf,
  newWindow,
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
})
