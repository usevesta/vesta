/**
 * The sample home of demo mode (`?demo=1`): what is in it, and a dashboard
 * for it that the screen editor can change (in memory only; reloading the
 * page starts over). Device state lives in `demo-store.ts`.
 */
import type { TFunction } from 'i18next'
import { create } from 'zustand'

import type { CapabilityKind, Dashboard, EndpointType, Target, Widget } from '@/api/generated'
import type { SaveDashboard } from '@/editor/editor-store'
import type { HomeItem } from '@/editor/model'
import type { DemoCoverId, DemoDoorId, DemoFanId, DemoLightId } from '@/demo/demo-store'
import type { IconName } from '@/lib/icons'

export type DemoThing =
  | { readonly kind: 'light'; readonly id: DemoLightId }
  | { readonly kind: 'fan'; readonly id: DemoFanId }
  | { readonly kind: 'cover'; readonly id: DemoCoverId; readonly icon: IconName; readonly sensitive: boolean }
  | { readonly kind: 'door'; readonly id: DemoDoorId }
  | { readonly kind: 'camera'; readonly unavailable: boolean }

type DemoName = keyof ReturnType<typeof demoNames>
type DemoRoom = keyof ReturnType<typeof demoRooms>

export interface DemoEntry {
  readonly target: Target
  readonly thing: DemoThing
  readonly name: DemoName
  /** The whole device, when this is one part of it. */
  readonly partOf?: DemoName
  readonly room: DemoRoom
  readonly type: EndpointType
  readonly capabilities: readonly CapabilityKind[]
}

function demoNames(t: TFunction) {
  return {
    livingRoomLight: t('demo.devices.livingRoomLight'),
    ceilingFan: t('demo.devices.ceilingFan'),
    fanLight: t('demo.devices.fanLight'),
    kitchenLight: t('demo.devices.kitchenLight'),
    hallwayLight: t('demo.devices.hallwayLight'),
    porchLight: t('demo.devices.porchLight'),
    bedroomBlind: t('demo.devices.bedroomBlind'),
    patioAwning: t('demo.devices.patioAwning'),
    garageDoor: t('demo.devices.garageDoor'),
    frontDoor: t('demo.devices.frontDoor'),
    frontDoorCamera: t('demo.devices.frontDoorCamera'),
    gardenCamera: t('demo.devices.gardenCamera'),
  }
}

function demoRooms(t: TFunction) {
  return {
    living_room: t('demo.rooms.livingRoom'),
    kitchen: t('demo.rooms.kitchen'),
    hallway: t('demo.rooms.hallway'),
    bedroom: t('demo.rooms.bedroom'),
    garage: t('demo.rooms.garage'),
    entrance: t('demo.rooms.entrance'),
    outside: t('demo.rooms.outside'),
  }
}

const LIGHT: readonly CapabilityKind[] = ['on_off', 'level']

export const DEMO_ENTRIES: readonly DemoEntry[] = [
  {
    target: 'living_room_light/main',
    thing: { kind: 'light', id: 'livingRoom' },
    name: 'livingRoomLight',
    room: 'living_room',
    type: 'light',
    capabilities: ['on_off', 'level', 'color_temperature'],
  },
  {
    target: 'ceiling_fan/fan',
    thing: { kind: 'fan', id: 'ceiling' },
    name: 'ceilingFan',
    room: 'living_room',
    type: 'fan',
    capabilities: ['on_off', 'fan'],
  },
  {
    target: 'ceiling_fan/light',
    thing: { kind: 'light', id: 'fanLight' },
    name: 'fanLight',
    partOf: 'ceilingFan',
    room: 'living_room',
    type: 'light',
    capabilities: LIGHT,
  },
  {
    target: 'kitchen_light/main',
    thing: { kind: 'light', id: 'kitchen' },
    name: 'kitchenLight',
    room: 'kitchen',
    type: 'light',
    capabilities: ['on_off'],
  },
  {
    target: 'hallway_light/main',
    thing: { kind: 'light', id: 'hallway' },
    name: 'hallwayLight',
    room: 'hallway',
    type: 'light',
    capabilities: LIGHT,
  },
  {
    target: 'bedroom_blind/main',
    thing: { kind: 'cover', id: 'bedroomBlind', icon: 'blind', sensitive: false },
    name: 'bedroomBlind',
    room: 'bedroom',
    type: 'window_covering',
    capabilities: ['cover'],
  },
  {
    target: 'garage_door/main',
    thing: { kind: 'cover', id: 'garage', icon: 'garage_door', sensitive: true },
    name: 'garageDoor',
    room: 'garage',
    type: 'garage_door',
    capabilities: ['cover'],
  },
  {
    target: 'front_door/main',
    thing: { kind: 'door', id: 'front' },
    name: 'frontDoor',
    room: 'entrance',
    type: 'door',
    capabilities: ['trigger'],
  },
  {
    target: 'front_door_camera/main',
    thing: { kind: 'camera', unavailable: false },
    name: 'frontDoorCamera',
    room: 'entrance',
    type: 'camera',
    capabilities: ['camera'],
  },
  {
    target: 'porch_light/main',
    thing: { kind: 'light', id: 'porch' },
    name: 'porchLight',
    room: 'outside',
    type: 'light',
    capabilities: ['on_off'],
  },
  {
    target: 'patio_awning/main',
    thing: { kind: 'cover', id: 'patioAwning', icon: 'window_covering', sensitive: false },
    name: 'patioAwning',
    room: 'outside',
    type: 'window_covering',
    capabilities: ['cover'],
  },
  {
    target: 'garden_camera/main',
    thing: { kind: 'camera', unavailable: true },
    name: 'gardenCamera',
    room: 'outside',
    type: 'camera',
    capabilities: ['camera'],
  },
]

export const DEMO_ENTRY_BY_TARGET: ReadonlyMap<Target, DemoEntry> = new Map(
  DEMO_ENTRIES.map((entry) => [entry.target, entry]),
)

/** The sample home's things, named in the current language. */
export function demoItems(t: TFunction): HomeItem[] {
  const names = demoNames(t)
  const rooms = demoRooms(t)
  return DEMO_ENTRIES.map((entry) => ({
    target: entry.target,
    name: names[entry.name],
    type: entry.type,
    capabilities: entry.capabilities,
    room: { id: entry.room, name: rooms[entry.room] },
    ...(entry.partOf === undefined ? {} : { partOf: names[entry.partOf] }),
  }))
}

export function demoName(entry: DemoEntry, t: TFunction): string {
  return demoNames(t)[entry.name]
}

function tile(type: string, target: Target | undefined, colSpan: number, rowSpan: number): Widget {
  return { type, col_span: colSpan, row_span: rowSpan, ...(target === undefined ? {} : { target }) }
}

/** The sample dashboard, with page titles in the current language. */
export function demoDashboard(t: TFunction): Dashboard {
  const clock: Widget = { ...tile('clock-weather', undefined, 2, 1), props: { label: t('demo.devices.clockPlace') } }
  return {
    schema_version: 1,
    theme: { preset: 'glass-dark', auto_dark_mode: true, touch_scale: 'large' },
    kiosk: { screensaver_timeout_sec: 120, wake_on: [] },
    pages: [
      {
        id: 'home',
        title: t('demo.pages.home'),
        icon: 'home',
        grid_columns: 4,
        widgets: [
          clock,
          tile('door-button', 'front_door/main', 2, 1),
          tile('camera-preview', 'front_door_camera/main', 2, 2),
          tile('light-control', 'living_room_light/main', 2, 1),
          tile('cover-control', 'garage_door/main', 2, 1),
        ],
      },
      {
        id: 'living_room',
        title: t('demo.pages.livingRoom'),
        icon: 'living_room',
        grid_columns: 4,
        widgets: [
          tile('light-control', 'living_room_light/main', 2, 1),
          tile('fan-control', 'ceiling_fan/fan', 2, 1),
          tile('light-control', 'ceiling_fan/light', 2, 1),
          tile('cover-control', 'bedroom_blind/main', 2, 1),
          tile('light-control', 'kitchen_light/main', 1, 1),
          tile('light-control', 'hallway_light/main', 1, 1),
        ],
      },
      {
        id: 'outside',
        title: t('demo.pages.outside'),
        icon: 'garden',
        grid_columns: 4,
        widgets: [
          tile('cover-control', 'garage_door/main', 2, 1),
          tile('cover-control', 'patio_awning/main', 2, 1),
          tile('light-control', 'porch_light/main', 2, 1),
          tile('camera-preview', 'garden_camera/main', 2, 2),
          // A widget type this version does not know: shows the placeholder.
          tile('thermostat-control', undefined, 2, 1),
        ],
      },
    ],
  }
}

export interface DemoDashboardState {
  readonly dashboard: Dashboard | null
  readonly revision: string
}

/** The sample dashboard as "saved" in this browser tab. */
export const useDemoDashboardStore = create<DemoDashboardState>()(() => ({ dashboard: null, revision: 'demo-1' }))

/** Pretend saving takes a moment, so "Saving…" is visible. */
const DEMO_SAVE_MS = 400

/** Saving in demo mode: same rules as the home (a stale revision is refused). */
export const saveDemoDashboard: SaveDashboard = async (dashboard, baseRevision) => {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, DEMO_SAVE_MS)
  })
  const current = useDemoDashboardStore.getState()
  if (baseRevision !== current.revision) return { ok: false, reason: 'conflict' }
  const next = Number(current.revision.replace('demo-', '')) + 1
  useDemoDashboardStore.setState({ dashboard, revision: `demo-${String(next)}` })
  return { ok: true }
}
