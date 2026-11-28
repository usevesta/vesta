import { describe, expect, it } from 'vitest'

import css from '@/index.css?raw'

/** The body of the first `selector { … }` block in the stylesheet. */
function block(selector: string): string {
  const start = css.indexOf(`${selector} {`)
  if (start < 0) throw new Error(`no ${selector} block`)
  const end = css.indexOf('\n}', start)
  return css.slice(start, end)
}

const PRESETS = [':root', '.dark', ':root[data-theme="glass-dark"]'] as const

describe('theme tokens', () => {
  it('define the contracted-power colors in every preset', () => {
    for (const preset of PRESETS) {
      const body = block(preset)
      for (const token of ['--warning', '--warning-strong']) {
        expect(body, `${preset} ${token}`).toMatch(new RegExp(`${token}: oklch\\(`))
      }
    }
  })

  it('expose them to components as semantic colors', () => {
    const theme = block('@theme inline')
    expect(theme).toContain('--color-warning: var(--warning);')
    expect(theme).toContain('--color-warning-strong: var(--warning-strong);')
  })
})
