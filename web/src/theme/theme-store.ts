import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'

import {
  effectivePreset,
  isColorSchemePreference,
  isThemePreset,
  type ColorSchemePreference,
  type ThemePreset,
} from '@/theme/theme'

export interface ThemeState {
  /** Style chosen on this screen; null follows the home's style. Remembered per screen. */
  presetOverride: ThemePreset | null
  /** The home's style from its dashboard (`theme.preset`). Not stored on the screen. */
  homePreset: ThemePreset | null
  /** Per-device light/dark override. `system` follows the OS. */
  colorScheme: ColorSchemePreference
  /** Translucency effects. Off adds `no-glass` on <html> (low-end tablets). */
  glass: boolean
  /** Pick a style for this screen, or null to use the home's style again. */
  setPresetOverride: (preset: ThemePreset | null) => void
  setHomePreset: (preset: ThemePreset | null) => void
  setColorScheme: (colorScheme: ColorSchemePreference) => void
  setGlass: (glass: boolean) => void
}

type PersistedTheme = Pick<ThemeState, 'presetOverride' | 'colorScheme' | 'glass'>

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export const useThemeStore = create<ThemeState>()(
  persist(
    (set) => ({
      presetOverride: null,
      homePreset: null,
      colorScheme: 'system',
      glass: true,
      setPresetOverride: (presetOverride) => {
        set({ presetOverride })
      },
      setHomePreset: (homePreset) => {
        set({ homePreset })
      },
      setColorScheme: (colorScheme) => {
        set({ colorScheme })
      },
      setGlass: (glass) => {
        set({ glass })
      },
    }),
    {
      name: 'vesta.display',
      // v1 stored a `preset` that was always set; it is dropped so screens
      // follow the home's style until someone picks one here.
      version: 2,
      migrate: (persisted) => persisted,
      storage: createJSONStorage(() => localStorage),
      partialize: (state): PersistedTheme => ({
        presetOverride: state.presetOverride,
        colorScheme: state.colorScheme,
        glass: state.glass,
      }),
      // Never trust storage: keep only well-formed values.
      merge: (persisted, current) => {
        if (!isRecord(persisted)) return current
        return {
          ...current,
          presetOverride: isThemePreset(persisted.presetOverride) ? persisted.presetOverride : null,
          colorScheme: isColorSchemePreference(persisted.colorScheme)
            ? persisted.colorScheme
            : current.colorScheme,
          glass: typeof persisted.glass === 'boolean' ? persisted.glass : current.glass,
        }
      },
    },
  ),
)

/** The preset this screen renders right now. */
export function selectPreset(state: Pick<ThemeState, 'presetOverride' | 'homePreset'>): ThemePreset {
  return effectivePreset(state.presetOverride, state.homePreset)
}
