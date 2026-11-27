/**
 * The screen editor's model: a draft of the dashboard and the edits a
 * person can make to it (move, resize, rename, remove and add tiles; add,
 * rename, reorder and remove pages). Pure functions, no React: the UI
 * dispatches `EditAction`s and saves `fromDraft(draft)`.
 *
 * Widgets have no id in `dashboard.json`; the draft gives each one a key
 * so the screen can follow a tile while it moves.
 */
import type { CapabilityKind, Dashboard, EndpointType, Page, RoomId, Target, Widget } from '@/api/generated'
import { EXTRA_WIDGET_TYPES, pickWidgetType, widgetMeta, type WidgetType } from '@/widgets/registry'
import { presetSpan, type SizePreset } from '@/widgets/sizes'

/** Limits the home enforces (see `vesta-core`'s `dashboard.rs`). */
export const MAX_PAGES = 32
export const MAX_WIDGETS_PER_PAGE = 64
export const MAX_TITLE_CHARS = 64
/** Names on tiles: long enough for "Luz de la habitación de invitados". */
export const MAX_LABEL_CHARS = 40
/** Columns of a new page when there is no other page to copy them from. */
export const DEFAULT_COLUMNS = 4

export interface DraftWidget {
  readonly key: string
  readonly widget: Widget
}

export interface DraftPage extends Omit<Page, 'widgets'> {
  readonly widgets: readonly DraftWidget[]
}

export interface Draft {
  /** The dashboard the edit started from (theme, kiosk and doorbell are kept as they are). */
  readonly base: Dashboard
  readonly pages: readonly DraftPage[]
  /** Next tile key; keys only need to be unique within one draft. */
  readonly nextKey: number
}

export function toDraft(dashboard: Dashboard): Draft {
  let nextKey = 1
  const pages = dashboard.pages.map(({ widgets, ...page }) => ({
    ...page,
    widgets: widgets.map((widget) => ({ key: `w${String(nextKey++)}`, widget })),
  }))
  return { base: dashboard, pages, nextKey }
}

export function fromDraft(draft: Draft): Dashboard {
  return {
    ...draft.base,
    pages: draft.pages.map(({ widgets, ...page }) => ({ ...page, widgets: widgets.map(({ widget }) => widget) })),
  }
}

/** The draft differs from what the edit started from. */
export function isDirty(draft: Draft): boolean {
  return JSON.stringify(fromDraft(draft)) !== JSON.stringify(draft.base)
}

export type MoveBy = -1 | 1

export type EditAction =
  | { readonly type: 'moveWidget'; readonly pageId: string; readonly key: string; readonly by: MoveBy }
  | { readonly type: 'resizeWidget'; readonly pageId: string; readonly key: string; readonly size: SizePreset }
  | { readonly type: 'renameWidget'; readonly pageId: string; readonly key: string; readonly label: string }
  | { readonly type: 'removeWidget'; readonly pageId: string; readonly key: string }
  /** Undo a removal: put the tile back where it was (or as close as possible). */
  | { readonly type: 'restoreWidget'; readonly pageId: string; readonly index: number; readonly widget: DraftWidget }
  | { readonly type: 'addWidget'; readonly pageId: string; readonly widget: Widget }
  | { readonly type: 'addPage'; readonly id: string; readonly title: string; readonly icon: string }
  | { readonly type: 'editPage'; readonly pageId: string; readonly title: string; readonly icon: string }
  | { readonly type: 'movePage'; readonly pageId: string; readonly by: MoveBy }
  | { readonly type: 'removePage'; readonly pageId: string }

/** At most `max` code points, as the home counts them (Rust `chars()`). */
function clampText(text: string, max: number): string {
  return Array.from(text.trim()).slice(0, max).join('')
}

function move<T>(items: readonly T[], index: number, by: MoveBy): readonly T[] {
  const target = index + by
  if (index < 0 || target < 0 || target >= items.length) return items
  const next = [...items]
  const [item] = next.splice(index, 1)
  if (item !== undefined) next.splice(target, 0, item)
  return next
}

/** `widget` with `props.label` set, or removed when `label` is empty (no empty `props` left behind). */
function withLabel(widget: Widget, label: string): Widget {
  const rest = Object.fromEntries(Object.entries(widget.props ?? {}).filter(([key]) => key !== 'label'))
  const props = label === '' ? rest : { ...rest, label }
  const next: Widget = { ...widget, props }
  if (Object.keys(props).length === 0) delete next.props
  return next
}

/** Apply `update` to one page; unknown page ids change nothing. */
function updatePage(draft: Draft, pageId: string, update: (page: DraftPage) => DraftPage): Draft {
  const index = draft.pages.findIndex((page) => page.id === pageId)
  const page = draft.pages[index]
  if (page === undefined) return draft
  const next = update(page)
  if (next === page) return draft
  const pages = [...draft.pages]
  pages[index] = next
  return { ...draft, pages }
}

/** Apply `update` to one tile; unknown pages or keys change nothing. */
function updateWidget(draft: Draft, pageId: string, key: string, update: (widget: Widget, page: DraftPage) => Widget): Draft {
  return updatePage(draft, pageId, (page) => {
    const index = page.widgets.findIndex((entry) => entry.key === key)
    const entry = page.widgets[index]
    if (entry === undefined) return page
    const widget = update(entry.widget, page)
    if (widget === entry.widget) return page
    const widgets = [...page.widgets]
    widgets[index] = { ...entry, widget }
    return { ...page, widgets }
  })
}

export function applyEdit(draft: Draft, action: EditAction): Draft {
  switch (action.type) {
    case 'moveWidget':
      return updatePage(draft, action.pageId, (page) => {
        const widgets = move(
          page.widgets,
          page.widgets.findIndex((entry) => entry.key === action.key),
          action.by,
        )
        return widgets === page.widgets ? page : { ...page, widgets }
      })

    case 'resizeWidget':
      return updateWidget(draft, action.pageId, action.key, (widget, page) => {
        const span = presetSpan(action.size, page.grid_columns)
        if (widget.col_span === span.colSpan && widget.row_span === span.rowSpan) return widget
        return { ...widget, col_span: span.colSpan, row_span: span.rowSpan }
      })

    case 'renameWidget':
      return updateWidget(draft, action.pageId, action.key, (widget) => {
        const label = clampText(action.label, MAX_LABEL_CHARS)
        const current = widget.props?.label
        if ((label === '' && current === undefined) || label === current) return widget
        return withLabel(widget, label)
      })

    case 'removeWidget':
      return updatePage(draft, action.pageId, (page) => {
        const widgets = page.widgets.filter((entry) => entry.key !== action.key)
        return widgets.length === page.widgets.length ? page : { ...page, widgets }
      })

    case 'restoreWidget':
      return updatePage(draft, action.pageId, (page) => {
        if (page.widgets.some((entry) => entry.key === action.widget.key)) return page
        if (page.widgets.length >= MAX_WIDGETS_PER_PAGE) return page
        const widgets = [...page.widgets]
        widgets.splice(Math.max(0, Math.min(action.index, widgets.length)), 0, action.widget)
        return { ...page, widgets }
      })

    case 'addWidget': {
      const key = `w${String(draft.nextKey)}`
      const next = updatePage(draft, action.pageId, (page) => {
        if (page.widgets.length >= MAX_WIDGETS_PER_PAGE) return page
        const columns = page.grid_columns
        // Never wider than the page.
        const widget = { ...action.widget, col_span: Math.max(1, Math.min(action.widget.col_span, columns)) }
        return { ...page, widgets: [...page.widgets, { key, widget }] }
      })
      return next === draft ? draft : { ...next, nextKey: draft.nextKey + 1 }
    }

    case 'addPage': {
      const title = clampText(action.title, MAX_TITLE_CHARS)
      if (draft.pages.length >= MAX_PAGES || title === '' || draft.pages.some((page) => page.id === action.id)) {
        return draft
      }
      const page: DraftPage = {
        id: action.id,
        title,
        icon: action.icon,
        grid_columns: draft.pages[0]?.grid_columns ?? DEFAULT_COLUMNS,
        widgets: [],
      }
      return { ...draft, pages: [...draft.pages, page] }
    }

    case 'editPage':
      return updatePage(draft, action.pageId, (page) => {
        const title = clampText(action.title, MAX_TITLE_CHARS)
        const next = { ...page, title: title === '' ? page.title : title, icon: action.icon }
        return next.title === page.title && next.icon === page.icon ? page : next
      })

    case 'movePage': {
      const pages = move(
        draft.pages,
        draft.pages.findIndex((page) => page.id === action.pageId),
        action.by,
      )
      return pages === draft.pages ? draft : { ...draft, pages }
    }

    case 'removePage': {
      const pages = draft.pages.filter((page) => page.id !== action.pageId)
      return pages.length === draft.pages.length ? draft : { ...draft, pages }
    }
  }
}

/** A page id that is not taken yet: `page_2`, `page_3`, … (snake_case, as the home requires). */
export function newPageId(draft: Draft): string {
  const taken = new Set(draft.pages.map((page) => page.id))
  for (let n = draft.pages.length + 1; ; n += 1) {
    const id = `page_${String(n)}`
    if (!taken.has(id)) return id
  }
}

// ---------------------------------------------------------------------------
// What can be added
// ---------------------------------------------------------------------------

export interface HomeRoom {
  readonly id: RoomId
  /** What people call it, e.g. "Living room". */
  readonly name: string
}

/** One thing in the home that a tile can show (an endpoint, in plain words). */
export interface HomeItem {
  readonly target: Target
  /** e.g. "Fan light". */
  readonly name: string
  /** The whole device's name, when this is one part of it (the fan's light). */
  readonly partOf?: string
  readonly room?: HomeRoom
  readonly type: EndpointType
  readonly capabilities: readonly CapabilityKind[]
}

export interface RoomGroup {
  /** Null: things without a room. */
  readonly room: HomeRoom | null
  readonly items: readonly HomeItem[]
}

/** `living_room` → "Living room". Until rooms have names of their own. */
export function roomNameFromId(id: RoomId): string {
  const words = id.split('_').filter((word) => word !== '')
  const text = words.join(' ')
  return text.charAt(0).toLocaleUpperCase() + text.slice(1)
}

/**
 * Items that some tile can show, grouped by room: rooms in the order they
 * first appear, things without a room last.
 */
export function groupByRoom(items: readonly HomeItem[]): RoomGroup[] {
  const groups = new Map<string | null, { room: HomeRoom | null; items: HomeItem[] }>()
  for (const item of items) {
    if (widgetForItem(item, DEFAULT_COLUMNS) === null) continue
    const key = item.room?.id ?? null
    let group = groups.get(key)
    if (group === undefined) {
      group = { room: item.room ?? null, items: [] }
      groups.set(key, group)
    }
    group.items.push(item)
  }
  const ordered = [...groups.values()]
  return [...ordered.filter((group) => group.room !== null), ...ordered.filter((group) => group.room === null)]
}

function sized(type: WidgetType, columns: number): Pick<Widget, 'col_span' | 'row_span'> {
  const span = presetSpan(widgetMeta[type].defaultSize, columns)
  return { col_span: span.colSpan, row_span: span.rowSpan }
}

/** The tile for `item`, chosen by its type and capabilities; null if none can show it. */
export function widgetForItem(item: HomeItem, columns: number): Widget | null {
  const type = pickWidgetType(item.type, item.capabilities)
  return type === null ? null : { type, target: item.target, ...sized(type, columns) }
}

/** A tile that shows no device (the clock). */
export function extraWidget(type: WidgetType, columns: number): Widget | null {
  return EXTRA_WIDGET_TYPES.includes(type) ? { type, ...sized(type, columns) } : null
}

/** The custom name on a tile, if it has one. */
export function widgetLabel(widget: Widget): string | undefined {
  const label = widget.props?.label
  return typeof label === 'string' && label.trim() !== '' ? label : undefined
}
