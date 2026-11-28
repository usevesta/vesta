/**
 * Pure helpers for the plug tile: how to say a power, energy, voltage or
 * current reading in plain numbers. No React, so it is unit tested.
 */

function format(value: number, language: string, digits: number): string {
  return new Intl.NumberFormat(language, { maximumFractionDigits: digits, minimumFractionDigits: 0 }).format(value)
}

export interface Reading {
  /** The number, formatted for `language`. */
  readonly value: string
  readonly unit: 'W' | 'kW' | 'kWh' | 'V' | 'A'
}

/** Watts below 1 kW (one decimal under 100 W), kilowatts above. */
export function powerReading(watts: number, language: string): Reading {
  const abs = Math.abs(watts)
  if (abs >= 1_000) return { value: format(watts / 1_000, language, 2), unit: 'kW' }
  return { value: format(watts, language, abs < 100 ? 1 : 0), unit: 'W' }
}

/** Kilowatt-hours: two decimals under 10 kWh, one under 100, then whole. */
export function energyReading(wattHours: number, language: string): Reading {
  const kwh = wattHours / 1_000
  const abs = Math.abs(kwh)
  return { value: format(kwh, language, abs < 10 ? 2 : abs < 100 ? 1 : 0), unit: 'kWh' }
}

export function voltageReading(volts: number, language: string): Reading {
  return { value: format(volts, language, 0), unit: 'V' }
}

/** Amps with two decimals (a lamp draws a few hundredths). */
export function currentReading(amps: number, language: string): Reading {
  return { value: format(amps, language, 2), unit: 'A' }
}

/** Keeps a number and its unit on one line. */
const NO_BREAK_SPACE = String.fromCharCode(0xa0)

/** "12.5 W" / "12,5 W" as one string. */
export function readingText(reading: Reading): string {
  return `${reading.value}${NO_BREAK_SPACE}${reading.unit}`
}

/** Below this the plug counts as drawing nothing (standby noise). */
export const IDLE_POWER_W = 0.5
