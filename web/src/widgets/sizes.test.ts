import { describe, expect, it } from 'vitest'

import { nextSize, presetSpan, sizeOf, sizesFor } from '@/widgets/sizes'

describe('tile sizes', () => {
  it('are relative to the page columns', () => {
    expect(presetSpan('small', 4)).toEqual({ colSpan: 1, rowSpan: 1 })
    expect(presetSpan('wide', 4)).toEqual({ colSpan: 2, rowSpan: 1 })
    expect(presetSpan('large', 4)).toEqual({ colSpan: 2, rowSpan: 2 })
    expect(presetSpan('full', 4)).toEqual({ colSpan: 4, rowSpan: 1 })
    expect(presetSpan('full', 6)).toEqual({ colSpan: 6, rowSpan: 1 })
    expect(presetSpan('wide', 1)).toEqual({ colSpan: 1, rowSpan: 1 })
    expect(presetSpan('full', 0)).toEqual({ colSpan: 1, rowSpan: 1 })
  })

  it('offer only distinct choices', () => {
    expect(sizesFor(4)).toEqual(['small', 'wide', 'large', 'full'])
    expect(sizesFor(3)).toEqual(['small', 'wide', 'large', 'full'])
    expect(sizesFor(2)).toEqual(['small', 'wide', 'large'])
    expect(sizesFor(1)).toEqual(['small', 'large'])
  })

  it('recognise the current preset, or none for other sizes', () => {
    expect(sizeOf({ colSpan: 2, rowSpan: 2 }, 4)).toBe('large')
    expect(sizeOf({ colSpan: 4, rowSpan: 1 }, 4)).toBe('full')
    expect(sizeOf({ colSpan: 2, rowSpan: 1 }, 2)).toBe('wide')
    expect(sizeOf({ colSpan: 3, rowSpan: 1 }, 4)).toBeNull()
  })

  it('cycle through the choices', () => {
    expect(nextSize({ colSpan: 1, rowSpan: 1 }, 4)).toBe('wide')
    expect(nextSize({ colSpan: 2, rowSpan: 1 }, 4)).toBe('large')
    expect(nextSize({ colSpan: 2, rowSpan: 2 }, 4)).toBe('full')
    expect(nextSize({ colSpan: 4, rowSpan: 1 }, 4)).toBe('small')
    expect(nextSize({ colSpan: 3, rowSpan: 4 }, 4)).toBe('small')
    expect(nextSize({ colSpan: 2, rowSpan: 2 }, 2)).toBe('small')
  })
})
