import { describe, expect, it } from 'vitest'

import { pickLanguage } from '@/i18n/language'
import { en } from '@/i18n/locales/en'
import { es } from '@/i18n/locales/es'

interface Tree {
  readonly [key: string]: string | Tree
}

function flatten(tree: Tree, prefix = ''): Map<string, string> {
  const out = new Map<string, string>()
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix === '' ? key : `${prefix}.${key}`
    if (typeof value === 'string') out.set(path, value)
    else for (const [k, v] of flatten(value, path)) out.set(k, v)
  }
  return out
}

const EN = flatten(en)
const ES = flatten(es)

describe('translations', () => {
  it('have the same keys in every language', () => {
    expect([...ES.keys()].sort()).toEqual([...EN.keys()].sort())
  })

  it('keep the same {{placeholders}} in every language', () => {
    const placeholders = (text: string) => [...text.matchAll(/{{\s*(\w+)\s*}}/g)].map((m) => m[1]).sort()
    for (const [key, text] of EN) {
      expect(placeholders(ES.get(key) ?? ''), key).toEqual(placeholders(text))
    }
  })

  it('never use technical words the household should not see', () => {
    // AGENTS.md glossary: these are never shown in the UI.
    const forbidden = [
      /\bhub\b/i,
      /\bservers?\b/i,
      /\bservidor(es)?\b/i,
      /\bip\b/i,
      /\bdrivers?\b/i,
      /\bcontrolador(es)?\b/i,
      /\bcapabilit(y|ies)\b/i,
      /\bendpoints?\b/i,
      /\btokens?\b/i,
      /\brelays?\b/i,
      /\btunnels?\b/i,
      /\btúnel(es)?\b/i,
      /\bports?\b/i,
      /\bpuertos?\b/i,
      /\bwebsockets?\b/i,
      /\bapi\b/i,
      /\bjson\b/i,
      /\bwidgets?\b/i,
    ]
    for (const [key, text] of [...EN, ...ES]) {
      for (const pattern of forbidden) {
        expect(pattern.test(text), `${key}: "${text}" matches ${String(pattern)}`).toBe(false)
      }
    }
  })
})

describe('pickLanguage', () => {
  it('picks the first supported language', () => {
    expect(pickLanguage(['fr-FR', 'es-ES', 'en-US'])).toBe('es')
    expect(pickLanguage([null, undefined, 'EN_gb'])).toBe('en')
  })

  it('falls back to English', () => {
    expect(pickLanguage(['de', 'fr'])).toBe('en')
    expect(pickLanguage([])).toBe('en')
  })
})
