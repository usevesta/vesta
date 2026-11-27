/**
 * Theme model: a preset (`data-theme` on <html>) plus a per-device color
 * scheme preference. Pure functions only, so they can be unit tested.
 */

export const THEME_PRESETS = ['default', 'glass-dark'] as const
export type ThemePreset = (typeof THEME_PRESETS)[number]

/** Used when neither this screen nor the home picked a style. */
export const DEFAULT_PRESET: ThemePreset = 'default'

export const COLOR_SCHEME_PREFERENCES = ['system', 'light', 'dark'] as const
export type ColorSchemePreference = (typeof COLOR_SCHEME_PREFERENCES)[number]

export type ResolvedColorScheme = 'light' | 'dark'

/** Which color schemes each preset can render. */
const PRESET_SCHEMES: Record<ThemePreset, readonly ResolvedColorScheme[]> = {
  default: ['light', 'dark'],
  'glass-dark': ['dark'],
}

export function isThemePreset(value: unknown): value is ThemePreset {
  return typeof value === 'string' && (THEME_PRESETS as readonly string[]).includes(value)
}

export function isColorSchemePreference(value: unknown): value is ColorSchemePreference {
  return (
    typeof value === 'string' && (COLOR_SCHEME_PREFERENCES as readonly string[]).includes(value)
  )
}

/**
 * The preset to render. The home's style (`dashboard.theme.preset`) is the
 * household default; a choice made on this screen overrides it here only.
 */
export function effectivePreset(screenOverride: ThemePreset | null, homePreset: ThemePreset | null): ThemePreset {
  return screenOverride ?? homePreset ?? DEFAULT_PRESET
}

/** True when the user can pick light or dark for this preset. */
export function presetSupportsSchemeChoice(preset: ThemePreset): boolean {
  return PRESET_SCHEMES[preset].length > 1
}

/**
 * Resolve the scheme to render. `system` follows `prefers-color-scheme`;
 * a manual choice wins; presets that only exist in one scheme force it.
 */
export function resolveColorScheme(
  preset: ThemePreset,
  preference: ColorSchemePreference,
  systemPrefersDark: boolean,
): ResolvedColorScheme {
  const supported = PRESET_SCHEMES[preset]
  const wanted: ResolvedColorScheme =
    preference === 'system' ? (systemPrefersDark ? 'dark' : 'light') : preference
  if (supported.includes(wanted)) return wanted
  return supported[0] ?? 'light'
}
