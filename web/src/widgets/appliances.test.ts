import { describe, expect, it } from 'vitest'

import {
  currentReading,
  energyReading,
  powerReading,
  readingText,
  voltageReading,
} from '@/widgets/outlet-control/readings'
import { availableActions, isBusy, isHome, splitMinutes, statusKey } from '@/widgets/robot-vacuum/robot'

describe('plug readings', () => {
  it('say power in W, and in kW from 1 kW', () => {
    expect(powerReading(12.5, 'en')).toEqual({ value: '12.5', unit: 'W' })
    expect(powerReading(0, 'en')).toEqual({ value: '0', unit: 'W' })
    expect(powerReading(125.4, 'en')).toEqual({ value: '125', unit: 'W' })
    expect(powerReading(1_540, 'en')).toEqual({ value: '1.54', unit: 'kW' })
    expect(powerReading(12.5, 'es').value).toBe('12,5')
  })

  it('say energy in kWh, voltage in V and current in A', () => {
    expect(energyReading(18_400, 'en')).toEqual({ value: '18.4', unit: 'kWh' })
    expect(energyReading(1_234, 'en')).toEqual({ value: '1.23', unit: 'kWh' })
    expect(energyReading(212_000, 'en')).toEqual({ value: '212', unit: 'kWh' })
    expect(voltageReading(227.2, 'en')).toEqual({ value: '227', unit: 'V' })
    expect(currentReading(0.112, 'es')).toEqual({ value: '0,11', unit: 'A' })
  })

  it('keep the number and the unit together', () => {
    expect(readingText({ value: '12.5', unit: 'W' })).toBe(`12.5${String.fromCharCode(0xa0)}W`)
  })
})

describe('robot vacuum', () => {
  it('says every status in words', () => {
    expect(statusKey('returning')).toBe('robot.status.returning')
    expect(statusKey(undefined)).toBeNull()
  })

  it('offers only the buttons that make sense now', () => {
    expect(availableActions('docked')).toEqual({ start: true, pause: false, dock: false, resumes: false })
    expect(availableActions('cleaning')).toEqual({ start: false, pause: true, dock: true, resumes: false })
    expect(availableActions('paused')).toEqual({ start: true, pause: false, dock: true, resumes: true })
    expect(availableActions('returning')).toEqual({ start: true, pause: true, dock: false, resumes: false })
    expect(availableActions('error')).toEqual({ start: true, pause: false, dock: true, resumes: false })
    expect(availableActions(undefined)).toEqual({ start: false, pause: false, dock: false, resumes: false })
  })

  it('knows when it is out and when it is home', () => {
    expect(isBusy('cleaning') && isBusy('returning')).toBe(true)
    expect(isBusy('paused')).toBe(false)
    expect(isHome('charging') && isHome('docked')).toBe(true)
    expect(isHome('idle')).toBe(false)
  })

  it('rounds the cleaning time to minutes', () => {
    expect(splitMinutes(1_800)).toEqual({ hours: 0, minutes: 30 })
    expect(splitMinutes(3_930)).toEqual({ hours: 1, minutes: 6 })
    expect(splitMinutes(-5)).toEqual({ hours: 0, minutes: 0 })
  })
})
