import { describe, expect, it } from 'vitest'

import { ICONS, isIconName, resolveIcon } from '@/lib/icons'

describe('icon registry', () => {
  it('resolves known semantic names', () => {
    expect(resolveIcon('fan')).toBe(ICONS.fan)
    expect(resolveIcon('ev_charger')).toBe(ICONS.ev_charger)
  })

  it('falls back for unknown or missing names', () => {
    expect(resolveIcon('spaceship')).toBe(ICONS.unknown)
    expect(resolveIcon(undefined)).toBe(ICONS.unknown)
    expect(resolveIcon(null)).toBe(ICONS.unknown)
    // Inherited object keys are not icons.
    expect(isIconName('toString')).toBe(false)
  })

  it('covers the names dashboards and endpoint types use', () => {
    const required = [
      'home',
      'light',
      'fan',
      'blind',
      'garage',
      'door',
      'lock',
      'camera',
      'doorbell',
      'sensor',
      'power',
      'ev_charger',
      'weather',
      'settings',
      'outlet',
      'switch',
      'window_covering',
      'garage_door',
      'gate',
      'meter',
    ]
    for (const name of required) {
      expect(isIconName(name), name).toBe(true)
    }
  })
})
