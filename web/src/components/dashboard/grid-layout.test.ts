import { describe, expect, it } from 'vitest'

import { clampSpan, effectiveColumns } from '@/components/dashboard/grid-layout'

describe('effectiveColumns', () => {
  it('keeps the requested columns when they fit', () => {
    expect(effectiveColumns(4, 1_200)).toBe(4)
    expect(effectiveColumns(4, 4 * 168)).toBe(4)
  })

  it('halves the columns on narrow screens (phones)', () => {
    expect(effectiveColumns(4, 600)).toBe(2)
    expect(effectiveColumns(4, 360)).toBe(2)
    expect(effectiveColumns(4, 300)).toBe(1)
    expect(effectiveColumns(6, 700)).toBe(3)
  })

  it('never exceeds what fits', () => {
    expect(effectiveColumns(3, 340)).toBe(2)
    expect(effectiveColumns(5, 520)).toBe(3)
  })

  it('uses the requested columns before the width is known', () => {
    expect(effectiveColumns(4, null)).toBe(4)
    expect(effectiveColumns(4, 0)).toBe(4)
  })

  it('sanitises invalid column counts', () => {
    expect(effectiveColumns(0, 1_000)).toBe(1)
    expect(effectiveColumns(Number.NaN, 1_000)).toBe(1)
    expect(effectiveColumns(2.7, 1_000)).toBe(2)
  })
})

describe('clampSpan', () => {
  it('clamps spans to the available columns', () => {
    expect(clampSpan(2, 4)).toBe(2)
    expect(clampSpan(4, 2)).toBe(2)
    expect(clampSpan(undefined, 4)).toBe(1)
    expect(clampSpan(0, 4)).toBe(1)
    expect(clampSpan(-2, 4)).toBe(1)
  })
})
