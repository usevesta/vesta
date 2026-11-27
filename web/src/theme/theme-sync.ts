import { useSyncExternalStore } from 'react'

import { resolveColorScheme, type ResolvedColorScheme } from '@/theme/theme'
import { selectPreset, useThemeStore } from '@/theme/theme-store'

const DARK_QUERY = '(prefers-color-scheme: dark)'

function darkMedia(): MediaQueryList | null {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia(DARK_QUERY)
    : null
}

/** Reflect the theme store onto <html>: `data-theme`, `.dark`, `.no-glass`. */
export function startThemeSync(root: HTMLElement = document.documentElement): () => void {
  const media = darkMedia()

  const apply = () => {
    const state = useThemeStore.getState()
    const { colorScheme, glass } = state
    const preset = selectPreset(state)
    const scheme = resolveColorScheme(preset, colorScheme, media?.matches ?? false)
    root.dataset.theme = preset
    root.classList.toggle('dark', scheme === 'dark')
    root.classList.toggle('no-glass', !glass)
  }

  apply()
  const unsubscribe = useThemeStore.subscribe(apply)
  media?.addEventListener('change', apply)
  return () => {
    unsubscribe()
    media?.removeEventListener('change', apply)
  }
}

function subscribeSystemScheme(onChange: () => void): () => void {
  const media = darkMedia()
  media?.addEventListener('change', onChange)
  return () => media?.removeEventListener('change', onChange)
}

function systemPrefersDark(): boolean {
  return darkMedia()?.matches ?? false
}

/** The scheme actually rendered right now (store + OS preference). */
export function useResolvedColorScheme(): ResolvedColorScheme {
  const prefersDark = useSyncExternalStore(subscribeSystemScheme, systemPrefersDark, () => false)
  const preset = useThemeStore(selectPreset)
  const colorScheme = useThemeStore((s) => s.colorScheme)
  return resolveColorScheme(preset, colorScheme, prefersDark)
}
