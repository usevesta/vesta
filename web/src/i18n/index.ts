import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import { FALLBACK_LANGUAGE, pickLanguage, SUPPORTED_LANGUAGES, type Language } from '@/i18n/language'
import { en } from '@/i18n/locales/en'
import { es } from '@/i18n/locales/es'

export const resources = {
  en: { translation: en },
  es: { translation: es },
} as const

const STORAGE_KEY = 'vesta.language'

function readStoredLanguage(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

/**
 * Initialise i18next synchronously (resources are bundled). Language order:
 * `?lang=` in the URL, the last choice made on this screen, the OS languages.
 */
export function initI18n(): typeof i18n {
  const params = new URLSearchParams(window.location.search)
  const language = pickLanguage([
    params.get('lang'),
    readStoredLanguage(),
    ...navigator.languages,
  ])

  void i18n.use(initReactI18next).init({
    resources,
    lng: language,
    fallbackLng: FALLBACK_LANGUAGE,
    supportedLngs: [...SUPPORTED_LANGUAGES],
    interpolation: { escapeValue: false }, // React escapes already.
    initAsync: false,
  })

  document.documentElement.lang = language
  i18n.on('languageChanged', (next: string) => {
    document.documentElement.lang = next
  })
  return i18n
}

/** Change the UI language and remember it for this screen. */
export function setLanguage(language: Language): void {
  try {
    localStorage.setItem(STORAGE_KEY, language)
  } catch {
    // Storage unavailable (private mode): the choice lasts for this session.
  }
  void i18n.changeLanguage(language)
}
