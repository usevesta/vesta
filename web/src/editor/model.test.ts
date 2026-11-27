import { describe, expect, it } from 'vitest'

import type { Dashboard } from '@/api/generated'
import {
  applyEdit,
  extraWidget,
  fromDraft,
  groupByRoom,
  isDirty,
  MAX_LABEL_CHARS,
  MAX_PAGES,
  MAX_TITLE_CHARS,
  MAX_WIDGETS_PER_PAGE,
  newPageId,
  roomNameFromId,
  toDraft,
  widgetForItem,
  type Draft,
  type EditAction,
  type HomeItem,
} from '@/editor/model'

function dashboard(): Dashboard {
  return {
    schema_version: 1,
    theme: { preset: 'glass-dark', auto_dark_mode: true, touch_scale: 'large' },
    kiosk: { screensaver_timeout_sec: 120, wake_on: [{ target: 'front_doorbell/main', event: 'doorbell.pressed' }] },
    pages: [
      {
        id: 'home',
        title: 'Home',
        icon: 'home',
        grid_columns: 4,
        widgets: [
          { type: 'clock-weather', col_span: 2, row_span: 1 },
          { type: 'light-control', target: 'living_room_light/main', col_span: 2, row_span: 1, props: { label: 'Lamp' } },
          { type: 'door-button', target: 'front_door_strike/main', col_span: 2, row_span: 1 },
        ],
      },
      { id: 'house', title: 'Blinds', grid_columns: 4, widgets: [] },
    ],
  }
}

function edit(draft: Draft, ...actions: EditAction[]): Draft {
  return actions.reduce(applyEdit, draft)
}

function types(draft: Draft, page = 0): string[] {
  return draft.pages[page]?.widgets.map(({ widget }) => widget.target ?? widget.type) ?? []
}

function keyAt(draft: Draft, index: number, page = 0): string {
  const key = draft.pages[page]?.widgets[index]?.key
  if (key === undefined) throw new Error(`no tile ${String(index)}`)
  return key
}

describe('draft', () => {
  it('round-trips the dashboard and keeps everything but the pages', () => {
    const draft = toDraft(dashboard())
    expect(fromDraft(draft)).toEqual(dashboard())
    expect(isDirty(draft)).toBe(false)
    expect(new Set(draft.pages.flatMap((page) => page.widgets.map(({ key }) => key))).size).toBe(3)
  })

  it('is not dirty after edits that change nothing', () => {
    const draft = toDraft(dashboard())
    const key = keyAt(draft, 0)
    expect(applyEdit(draft, { type: 'moveWidget', pageId: 'home', key, by: -1 })).toBe(draft)
    expect(applyEdit(draft, { type: 'resizeWidget', pageId: 'home', key, size: 'wide' })).toBe(draft)
    expect(applyEdit(draft, { type: 'removeWidget', pageId: 'nope', key })).toBe(draft)
    expect(applyEdit(draft, { type: 'renameWidget', pageId: 'home', key: keyAt(draft, 1), label: ' Lamp ' })).toBe(draft)
    // Moving there and back is a change of nothing.
    const back = edit(
      draft,
      { type: 'moveWidget', pageId: 'home', key, by: 1 },
      { type: 'moveWidget', pageId: 'home', key, by: -1 },
    )
    expect(isDirty(back)).toBe(false)
  })
})

describe('tiles', () => {
  it('move earlier and later, never past the ends', () => {
    const draft = toDraft(dashboard())
    const clock = keyAt(draft, 0)
    const later = applyEdit(draft, { type: 'moveWidget', pageId: 'home', key: clock, by: 1 })
    expect(types(later)).toEqual(['living_room_light/main', 'clock-weather', 'front_door_strike/main'])
    expect(keyAt(later, 1)).toBe(clock)
    const last = edit(later, { type: 'moveWidget', pageId: 'home', key: clock, by: 1 })
    expect(types(last).at(-1)).toBe('clock-weather')
    expect(applyEdit(last, { type: 'moveWidget', pageId: 'home', key: clock, by: 1 })).toBe(last)
    expect(isDirty(later)).toBe(true)
  })

  it('resize to presets relative to the page columns', () => {
    const draft = toDraft(dashboard())
    const lamp = keyAt(draft, 1)
    const large = applyEdit(draft, { type: 'resizeWidget', pageId: 'home', key: lamp, size: 'large' })
    expect(fromDraft(large).pages[0]?.widgets[1]).toMatchObject({ col_span: 2, row_span: 2 })
    const full = applyEdit(large, { type: 'resizeWidget', pageId: 'home', key: lamp, size: 'full' })
    expect(fromDraft(full).pages[0]?.widgets[1]).toMatchObject({ col_span: 4, row_span: 1 })
    const small = applyEdit(full, { type: 'resizeWidget', pageId: 'home', key: lamp, size: 'small' })
    expect(fromDraft(small).pages[0]?.widgets[1]).toMatchObject({ col_span: 1, row_span: 1 })
  })

  it('rename sets, trims, shortens and clears the label', () => {
    const draft = toDraft(dashboard())
    const door = keyAt(draft, 2)
    const named = applyEdit(draft, { type: 'renameWidget', pageId: 'home', key: door, label: '  Street door ' })
    expect(fromDraft(named).pages[0]?.widgets[2]?.props).toEqual({ label: 'Street door' })

    const long = applyEdit(draft, { type: 'renameWidget', pageId: 'home', key: door, label: 'é'.repeat(100) })
    expect(fromDraft(long).pages[0]?.widgets[2]?.props?.label).toBe('é'.repeat(MAX_LABEL_CHARS))

    // Empty goes back to the thing's own name, without leaving `props: {}`.
    const cleared = applyEdit(named, { type: 'renameWidget', pageId: 'home', key: door, label: '   ' })
    expect(fromDraft(cleared).pages[0]?.widgets[2]).toEqual(dashboard().pages[0]?.widgets[2])
    expect(isDirty(cleared)).toBe(false)

    // Other props survive a rename.
    const withProps = toDraft({
      ...dashboard(),
      pages: [{ id: 'cam', title: 'Cam', grid_columns: 4, widgets: [{ type: 'camera-preview', col_span: 2, row_span: 2, props: { live_webrtc: true } }] }],
    })
    const renamed = applyEdit(withProps, { type: 'renameWidget', pageId: 'cam', key: keyAt(withProps, 0), label: 'Door' })
    expect(fromDraft(renamed).pages[0]?.widgets[0]?.props).toEqual({ live_webrtc: true, label: 'Door' })
    const unnamed = applyEdit(renamed, { type: 'renameWidget', pageId: 'cam', key: keyAt(withProps, 0), label: '' })
    expect(fromDraft(unnamed).pages[0]?.widgets[0]?.props).toEqual({ live_webrtc: true })
  })

  it('remove, and undo puts the tile back where it was', () => {
    const draft = toDraft(dashboard())
    const entry = draft.pages[0]?.widgets[1]
    if (entry === undefined) throw new Error('missing tile')
    const removed = applyEdit(draft, { type: 'removeWidget', pageId: 'home', key: entry.key })
    expect(types(removed)).toEqual(['clock-weather', 'front_door_strike/main'])
    const restored = applyEdit(removed, { type: 'restoreWidget', pageId: 'home', index: 1, widget: entry })
    expect(fromDraft(restored)).toEqual(dashboard())
    // Restoring twice does nothing; an index past the end appends.
    expect(applyEdit(restored, { type: 'restoreWidget', pageId: 'home', index: 1, widget: entry })).toBe(restored)
    const appended = applyEdit(removed, { type: 'restoreWidget', pageId: 'home', index: 99, widget: entry })
    expect(types(appended).at(-1)).toBe('living_room_light/main')
  })

  it('add appends with a fresh key and never wider than the page', () => {
    const draft = toDraft(dashboard())
    const added = applyEdit(draft, {
      type: 'addWidget',
      pageId: 'house',
      widget: { type: 'cover-control', target: 'bedroom_blind/main', col_span: 9, row_span: 2 },
    })
    expect(fromDraft(added).pages[1]?.widgets).toEqual([
      { type: 'cover-control', target: 'bedroom_blind/main', col_span: 4, row_span: 2 },
    ])
    const allKeys = added.pages.flatMap((page) => page.widgets.map(({ key }) => key))
    expect(new Set(allKeys).size).toBe(allKeys.length)
    expect(applyEdit(draft, { type: 'addWidget', pageId: 'nope', widget: { type: 'clock-weather', col_span: 1, row_span: 1 } })).toBe(draft)
  })

  it('stops adding at the page limit', () => {
    let draft = toDraft({ ...dashboard(), pages: [{ id: 'home', title: 'Home', grid_columns: 4, widgets: [] }] })
    for (let i = 0; i < MAX_WIDGETS_PER_PAGE + 3; i += 1) {
      draft = applyEdit(draft, { type: 'addWidget', pageId: 'home', widget: { type: 'clock-weather', col_span: 1, row_span: 1 } })
    }
    expect(draft.pages[0]?.widgets).toHaveLength(MAX_WIDGETS_PER_PAGE)
  })
})

describe('pages', () => {
  it('add with a new snake_case id and the columns of the first page', () => {
    const draft = toDraft(dashboard())
    const id = newPageId(draft)
    expect(id).toBe('page_3')
    expect(id).toMatch(/^[a-z0-9]+(_[a-z0-9]+)*$/)
    const added = applyEdit(draft, { type: 'addPage', id, title: '  Kitchen ', icon: 'kitchen' })
    expect(fromDraft(added).pages[2]).toEqual({ id: 'page_3', title: 'Kitchen', icon: 'kitchen', grid_columns: 4, widgets: [] })
    expect(newPageId(added)).toBe('page_4')
    // Taken ids, empty titles and the page limit are refused.
    expect(applyEdit(added, { type: 'addPage', id: 'home', title: 'Again', icon: 'home' })).toBe(added)
    expect(applyEdit(added, { type: 'addPage', id: 'page_9', title: ' ', icon: 'home' })).toBe(added)
    let full = added
    for (let i = 0; i < MAX_PAGES + 2; i += 1) {
      full = applyEdit(full, { type: 'addPage', id: newPageId(full), title: `P${String(i)}`, icon: 'home' })
    }
    expect(full.pages).toHaveLength(MAX_PAGES)
  })

  it('never reuses an id that is taken', () => {
    const draft = toDraft({
      ...dashboard(),
      pages: [
        { id: 'page_2', title: 'A', grid_columns: 4, widgets: [] },
        { id: 'page_3', title: 'B', grid_columns: 4, widgets: [] },
      ],
    })
    expect(newPageId(draft)).toBe('page_4')
    const empty = toDraft({ ...dashboard(), pages: [] })
    expect(newPageId(empty)).toBe('page_1')
    expect(applyEdit(empty, { type: 'addPage', id: 'page_1', title: 'First', icon: 'home' }).pages[0]?.grid_columns).toBe(4)
  })

  it('rename and change the icon; an empty title keeps the old one', () => {
    const draft = toDraft(dashboard())
    const renamed = applyEdit(draft, { type: 'editPage', pageId: 'house', title: 'Persianas', icon: 'blind' })
    expect(fromDraft(renamed).pages[1]).toMatchObject({ title: 'Persianas', icon: 'blind' })
    const kept = applyEdit(renamed, { type: 'editPage', pageId: 'house', title: '', icon: 'blind' })
    expect(kept).toBe(renamed)
    const long = applyEdit(draft, { type: 'editPage', pageId: 'house', title: 'x'.repeat(200), icon: 'blind' })
    expect(fromDraft(long).pages[1]?.title).toHaveLength(MAX_TITLE_CHARS)
  })

  it('reorder and remove', () => {
    const draft = toDraft(dashboard())
    const moved = applyEdit(draft, { type: 'movePage', pageId: 'house', by: -1 })
    expect(moved.pages.map((page) => page.id)).toEqual(['house', 'home'])
    expect(applyEdit(moved, { type: 'movePage', pageId: 'house', by: -1 })).toBe(moved)
    const removed = applyEdit(moved, { type: 'removePage', pageId: 'home' })
    expect(fromDraft(removed).pages.map((page) => page.id)).toEqual(['house'])
    // The kiosk and doorbell settings are untouched.
    expect(fromDraft(removed).kiosk).toEqual(dashboard().kiosk)
  })
})

describe('what can be added', () => {
  const item = (target: string, type: HomeItem['type'], capabilities: HomeItem['capabilities'], room?: string): HomeItem => ({
    target,
    name: target,
    type,
    capabilities,
    ...(room === undefined ? {} : { room: { id: room, name: roomNameFromId(room) } }),
  })

  it('names rooms from their ids until rooms have names', () => {
    expect(roomNameFromId('living_room')).toBe('Living room')
    expect(roomNameFromId('garage')).toBe('Garage')
    expect(roomNameFromId('2nd_floor')).toBe('2nd floor')
  })

  it('groups by room in order, things without a room last, and skips what no tile can show', () => {
    const groups = groupByRoom([
      item('porch/main', 'light', ['on_off']),
      item('lamp/main', 'light', ['on_off'], 'living_room'),
      item('meter/main', 'meter', ['power_meter'], 'garage'),
      item('blind/main', 'window_covering', ['cover'], 'bedroom'),
      item('fan/fan', 'fan', ['on_off', 'fan'], 'living_room'),
    ])
    expect(groups.map((group) => group.room?.name ?? null)).toEqual(['Living room', 'Bedroom', null])
    expect(groups[0]?.items.map((entry) => entry.target)).toEqual(['lamp/main', 'fan/fan'])
  })

  it('builds the tile picked for the thing, with its default size', () => {
    expect(widgetForItem(item('blind/main', 'window_covering', ['cover']), 4)).toEqual({
      type: 'cover-control',
      target: 'blind/main',
      col_span: 2,
      row_span: 2,
    })
    expect(widgetForItem(item('strike/main', 'door', ['trigger']), 4)).toEqual({
      type: 'door-button',
      target: 'strike/main',
      col_span: 2,
      row_span: 1,
    })
    expect(widgetForItem(item('lamp/main', 'light', ['on_off', 'level']), 1)).toMatchObject({ col_span: 1, row_span: 1 })
    expect(widgetForItem(item('meter/main', 'meter', ['power_meter']), 4)).toBeNull()
    expect(extraWidget('clock-weather', 4)).toEqual({ type: 'clock-weather', col_span: 2, row_span: 1 })
    expect(extraWidget('light-control', 4)).toBeNull()
  })
})
