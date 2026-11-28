/**
 * Widget registry: maps the `type` string of a dashboard widget to the
 * component that renders it. Pages never hardcode widgets; they look them up
 * here (AGENTS.md rule 5). Unknown types render `UnknownWidget`.
 *
 * Each widget has its own presentational props (see the component files).
 * The integration layer maps generated API types onto them and builds a
 * `WidgetSpec` per tile.
 */
import type { ComponentType } from 'react'

import type { CapabilityKind, EndpointType } from '@/api/generated'
import type { IconName } from '@/lib/icons'
import { CameraPreview, type CameraPreviewProps } from '@/widgets/camera-preview/CameraPreview'
import { ClockWeather, type ClockWeatherProps } from '@/widgets/clock-weather/ClockWeather'
import { CoverControl, type CoverControlProps } from '@/widgets/cover-control/CoverControl'
import { DoorButton, type DoorButtonProps } from '@/widgets/door-button/DoorButton'
import { EvCharger, type EvChargerProps } from '@/widgets/ev-charger/EvCharger'
import { FanControl, type FanControlProps } from '@/widgets/fan-control/FanControl'
import { LightControl, type LightControlProps } from '@/widgets/light-control/LightControl'
import type { SizePreset } from '@/widgets/sizes'

/** Props of every registered widget, keyed by its `type` string. */
export interface WidgetPropsMap {
  'light-control': LightControlProps
  'fan-control': FanControlProps
  'cover-control': CoverControlProps
  'door-button': DoorButtonProps
  'camera-preview': CameraPreviewProps
  'ev-charger': EvChargerProps
  'clock-weather': ClockWeatherProps
}

export type WidgetType = keyof WidgetPropsMap

type WidgetRegistry = { readonly [K in WidgetType]: ComponentType<WidgetPropsMap[K]> }

export const widgetRegistry: WidgetRegistry = {
  'light-control': LightControl,
  'fan-control': FanControl,
  'cover-control': CoverControl,
  'door-button': DoorButton,
  'camera-preview': CameraPreview,
  'ev-charger': EvCharger,
  'clock-weather': ClockWeather,
}

export const WIDGET_TYPES = [
  'light-control',
  'fan-control',
  'cover-control',
  'door-button',
  'camera-preview',
  'ev-charger',
  'clock-weather',
] as const satisfies readonly WidgetType[]

/** A widget type plus props that match it (a discriminated union). */
export type WidgetSpec<K extends WidgetType = WidgetType> = {
  [P in K]: { type: P; props: WidgetPropsMap[P] }
}[K]

/** Anything that may come from a dashboard, including types we do not know. */
export interface AnyWidgetSpec {
  type: string
  props?: unknown
}

export function isWidgetType(type: string): type is WidgetType {
  return Object.hasOwn(widgetRegistry, type)
}

export function getWidgetComponent<K extends WidgetType>(type: K): ComponentType<WidgetPropsMap[K]> {
  return widgetRegistry[type]
}

/**
 * What the editor needs to know about a widget: which things in the home
 * it shows (by endpoint type, never by brand) and how big it starts.
 */
export interface WidgetMeta {
  /** Endpoint types this widget is made for. Empty: it shows no device (e.g. the clock). */
  readonly endpointTypes: readonly EndpointType[]
  /**
   * The endpoint must have at least one of these capabilities. Tells
   * widgets for the same type apart: a gate that moves (`cover`) gets
   * cover controls, one that opens with a pulse (`trigger`) a door button.
   */
  readonly needsAnyOf?: readonly CapabilityKind[]
  readonly defaultSize: SizePreset
  /** Icon in the editor when there is no device to take one from. */
  readonly icon: IconName
}

export const widgetMeta: Readonly<Record<WidgetType, WidgetMeta>> = {
  'light-control': {
    endpointTypes: ['light', 'outlet', 'switch'],
    needsAnyOf: ['on_off'],
    defaultSize: 'wide',
    icon: 'light',
  },
  'fan-control': { endpointTypes: ['fan'], needsAnyOf: ['fan'], defaultSize: 'wide', icon: 'fan' },
  'cover-control': {
    endpointTypes: ['window_covering', 'garage_door', 'gate'],
    needsAnyOf: ['cover'],
    defaultSize: 'large',
    icon: 'blind',
  },
  'door-button': {
    endpointTypes: ['door', 'lock', 'gate', 'garage_door'],
    needsAnyOf: ['trigger', 'lock'],
    defaultSize: 'wide',
    icon: 'door',
  },
  'camera-preview': { endpointTypes: ['camera', 'doorbell'], defaultSize: 'large', icon: 'camera' },
  // Large: the energy flow and every control fit; smaller sizes open them in a dialog.
  'ev-charger': { endpointTypes: ['ev_charger'], needsAnyOf: ['ev_charger'], defaultSize: 'large', icon: 'ev_charger' },
  'clock-weather': { endpointTypes: [], defaultSize: 'wide', icon: 'clock' },
}

/**
 * The widget that shows an endpoint of `type` with these capabilities,
 * or null if no widget can show it yet (e.g. sensors and meters; a
 * charger's meters show inside its own tile). The
 * first match in `WIDGET_TYPES` order wins.
 */
export function pickWidgetType(type: EndpointType, capabilities: readonly CapabilityKind[]): WidgetType | null {
  return (
    WIDGET_TYPES.find((widget) => {
      const meta = widgetMeta[widget]
      if (!meta.endpointTypes.includes(type)) return false
      return meta.needsAnyOf === undefined || meta.needsAnyOf.some((kind) => capabilities.includes(kind))
    }) ?? null
  )
}

/** Widgets that show no device and can go on any page (the clock). */
export const EXTRA_WIDGET_TYPES: readonly WidgetType[] = WIDGET_TYPES.filter(
  (type) => widgetMeta[type].endpointTypes.length === 0,
)
