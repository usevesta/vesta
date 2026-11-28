/**
 * Tile sizes the editor offers. People pick one of a few presets instead
 * of resizing freely (easier on a wall, and pages stay tidy). A preset is
 * relative to the page's columns, so "full width" is 4 columns on a
 * 4-column page and 2 on a 2-column one.
 */

export const SIZE_PRESETS = ['small', 'wide', 'large', 'full'] as const
export type SizePreset = (typeof SIZE_PRESETS)[number]

export interface Span {
  readonly colSpan: number
  readonly rowSpan: number
}

function columnsOf(columns: number): number {
  return Number.isFinite(columns) ? Math.max(1, Math.floor(columns)) : 1
}

/** Columns × rows of `preset` on a page with `columns` columns. */
export function presetSpan(preset: SizePreset, columns: number): Span {
  const available = columnsOf(columns)
  switch (preset) {
    case 'small':
      return { colSpan: 1, rowSpan: 1 }
    case 'wide':
      return { colSpan: Math.min(2, available), rowSpan: 1 }
    case 'large':
      return { colSpan: Math.min(2, available), rowSpan: 2 }
    case 'full':
      return { colSpan: available, rowSpan: 1 }
  }
}

/**
 * The presets that make sense on a page with `columns` columns, smallest
 * first and without duplicates (on a 2-column page "wide" already is the
 * full width).
 */
export function sizesFor(columns: number): SizePreset[] {
  const seen = new Set<string>()
  const sizes: SizePreset[] = []
  for (const preset of SIZE_PRESETS) {
    const span = presetSpan(preset, columns)
    const key = `${String(span.colSpan)}x${String(span.rowSpan)}`
    if (seen.has(key)) continue
    seen.add(key)
    sizes.push(preset)
  }
  return sizes
}

/** The preset a tile currently has; null for a size made some other way. */
export function sizeOf(span: Span, columns: number): SizePreset | null {
  return (
    sizesFor(columns).find((preset) => {
      const candidate = presetSpan(preset, columns)
      return candidate.colSpan === span.colSpan && candidate.rowSpan === span.rowSpan
    }) ?? null
  )
}

/** The next preset when the size button is tapped again (wraps around). */
export function nextSize(span: Span, columns: number): SizePreset {
  const sizes = sizesFor(columns)
  const current = sizeOf(span, columns)
  const index = current === null ? -1 : sizes.indexOf(current)
  return sizes[(index + 1) % sizes.length] ?? 'small'
}
