import {
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from 'react'

import { clampSpan, DEFAULT_MIN_COLUMN_WIDTH, effectiveColumns, packsTiles } from '@/components/dashboard/grid-layout'
import { cn } from '@/lib/utils'

const GridColumnsContext = createContext(1)

export interface DashboardGridProps {
  /** Columns the page was designed for (the page's `grid_columns`). */
  columns: number
  /** Narrowest column before the grid collapses, in px. */
  minColumnWidth?: number
  /** Minimum row height (CSS length). Rows grow to fit their content. */
  rowHeight?: string
  /** Never fill gaps with later tiles (the editor: every move must be visible). */
  keepOrder?: boolean
  className?: string
  children?: ReactNode
}

/** Content width of the returned ref's element, kept up to date with a ResizeObserver. */
function useElementWidth(): [RefObject<HTMLDivElement | null>, number | null] {
  const ref = useRef<HTMLDivElement | null>(null)
  const [width, setWidth] = useState<number | null>(null)
  useLayoutEffect(() => {
    const element = ref.current
    if (element === null || typeof ResizeObserver === 'undefined') return
    // Measure before the first paint so phones never flash the wide layout.
    setWidth(element.getBoundingClientRect().width)
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (entry !== undefined) setWidth(entry.contentRect.width)
    })
    observer.observe(element)
    return () => {
      observer.disconnect()
    }
  }, [])
  return [ref, width]
}

/**
 * Lays out tiles on a column grid. Collapses to fewer columns on narrow
 * screens; `DashboardGridItem` spans are clamped to the visible columns.
 */
export function DashboardGrid({
  columns,
  minColumnWidth = DEFAULT_MIN_COLUMN_WIDTH,
  rowHeight = '9rem',
  keepOrder = false,
  className,
  children,
}: DashboardGridProps) {
  const [ref, width] = useElementWidth()
  const visibleColumns = effectiveColumns(columns, width, minColumnWidth)
  const dense = !keepOrder && packsTiles(columns, visibleColumns)
  const style: CSSProperties = {
    gridTemplateColumns: `repeat(${String(visibleColumns)}, minmax(0, 1fr))`,
    gridAutoRows: `minmax(${rowHeight}, auto)`,
  }
  return (
    <GridColumnsContext value={visibleColumns}>
      <div
        ref={ref}
        data-slot="dashboard-grid"
        data-columns={visibleColumns}
        data-packed={dense || undefined}
        className={cn('grid gap-3 md:gap-4', dense ? 'grid-flow-row-dense' : 'grid-flow-row', className)}
        style={style}
      >
        {children}
      </div>
    </GridColumnsContext>
  )
}

export interface DashboardGridItemProps {
  /** Columns this tile spans (the widget's `col_span`). Default 1. */
  colSpan?: number
  /** Rows this tile spans (the widget's `row_span`). Default 1. */
  rowSpan?: number
  className?: string
  children?: ReactNode
}

export function DashboardGridItem({ colSpan, rowSpan, className, children }: DashboardGridItemProps) {
  const columns = useContext(GridColumnsContext)
  const style: CSSProperties = {
    gridColumn: `span ${String(clampSpan(colSpan, columns))} / span ${String(clampSpan(colSpan, columns))}`,
    gridRow: `span ${String(clampSpan(rowSpan, Number.POSITIVE_INFINITY))}`,
  }
  return (
    <div data-slot="dashboard-grid-item" className={cn('min-w-0', className)} style={style}>
      {children}
    </div>
  )
}
