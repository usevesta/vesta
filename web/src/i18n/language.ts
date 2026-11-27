export const SUPPORTED_LANGUAGES = ['en', 'es'] as const
export type Language = (typeof SUPPORTED_LANGUAGES)[number]

export const FALLBACK_LANGUAGE: Language = 'en'

export function isLanguage(value: unknown): value is Language {
  return typeof value === 'string' && (SUPPORTED_LANGUAGES as readonly string[]).includes(value)
}

/**
 * First supported language among `candidates` (BCP 47 tags such as
 * `es-ES`), in order of preference; `fallback` if none match.
 */
export function pickLanguage(
  candidates: readonly (string | null | undefined)[],
  fallback: Language = FALLBACK_LANGUAGE,
): Language {
  for (const candidate of candidates) {
    if (candidate == null) continue
    const base = candidate.trim().toLowerCase().split(/[-_]/)[0]
    if (isLanguage(base)) return base
  }
  return fallback
}
