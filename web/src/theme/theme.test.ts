import { describe, expect, it } from 'vitest'

import {
  effectivePreset,
  isColorSchemePreference,
  isThemePreset,
  presetSupportsSchemeChoice,
  resolveColorScheme,
} from '@/theme/theme'

describe('resolveColorScheme', () => {
  it('follows the system preference by default', () => {
    expect(resolveColorScheme('default', 'system', true)).toBe('dark')
    expect(resolveColorScheme('default', 'system', false)).toBe('light')
  })

  it('lets a manual choice override the system', () => {
    expect(resolveColorScheme('default', 'light', true)).toBe('light')
    expect(resolveColorScheme('default', 'dark', false)).toBe('dark')
  })

  it('forces dark for the glass-dark preset', () => {
    expect(resolveColorScheme('glass-dark', 'light', false)).toBe('dark')
    expect(resolveColorScheme('glass-dark', 'system', false)).toBe('dark')
    expect(presetSupportsSchemeChoice('glass-dark')).toBe(false)
    expect(presetSupportsSchemeChoice('default')).toBe(true)
  })
})

describe('guards', () => {
  it('accepts only known values', () => {
    expect(isThemePreset('glass-dark')).toBe(true)
    expect(isThemePreset('neon')).toBe(false)
    expect(isThemePreset(3)).toBe(false)
    expect(isColorSchemePreference('system')).toBe(true)
    expect(isColorSchemePreference('auto')).toBe(false)
  })
})

describe('effectivePreset', () => {
  it("uses the home's style unless this screen picked one", () => {
    expect(effectivePreset(null, null)).toBe('default')
    expect(effectivePreset(null, 'glass-dark')).toBe('glass-dark')
    expect(effectivePreset('default', 'glass-dark')).toBe('default')
    expect(effectivePreset('glass-dark', null)).toBe('glass-dark')
  })
})
