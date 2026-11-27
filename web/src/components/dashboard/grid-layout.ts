/**
 * Pure layout math for `DashboardGrid`.
 *
 * A page asks for N columns. When the grid is too narrow for N columns of at
 * least `minColumnWidth`, the column count is halved (4 -> 2 -> 1) so tile
 * proportions stay recognisable, and spans are clamped to what fits.
 */

/** Narrowest a column may get before the grid collapses, in px. */
export const DEFAULT_MIN_COLUMN_WIDTH = 168

function toPositiveInt(value: number | undefined, fallback: number): number {
  if (value === undefined || !Number.isFinite(value)) return fallback
  return Math.max(1, Math.floor(value))
}

export function effectiveColumns(
  requested: number,
  containerWidth: number | null,
  minColumnWidth: number = DEFAULT_MIN_COLUMN_WIDTH,
): number {
  let columns = toPositiveInt(requested, 1)
  if (containerWidth === null || containerWidth <= 0 || minColumnWidth <= 0) return columns
  const fits = Math.max(1, Math.floor(containerWidth / minColumnWidth))
  while (columns > fits && columns > 1) {
    columns = Math.ceil(columns / 2)
  }
  return Math.min(columns, Math.max(fits, 1))
}

/**
 * Fill gaps with later tiles only when the page is squeezed onto fewer
 * columns than it was arranged for (phones). At its own columns a page
 * keeps exactly the order it was arranged in, so the screen looks like
 * the editor showed it.
 */
export function packsTiles(requested: number, visible: number): boolean {
  return visible < toPositiveInt(requested, 1)
}

/** Clamp a requested span to `[1, columns]`. Missing or invalid spans are 1. */
export function clampSpan(span: number | undefined, columns: number): number {
  return Math.min(toPositiveInt(span, 1), Math.max(1, columns))
}
