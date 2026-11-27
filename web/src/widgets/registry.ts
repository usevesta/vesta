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

import { CameraPreview, type CameraPreviewProps } from '@/widgets/camera-preview/CameraPreview'
import { ClockWeather, type ClockWeatherProps } from '@/widgets/clock-weather/ClockWeather'
import { CoverControl, type CoverControlProps } from '@/widgets/cover-control/CoverControl'
import { DoorButton, type DoorButtonProps } from '@/widgets/door-button/DoorButton'
import { FanControl, type FanControlProps } from '@/widgets/fan-control/FanControl'
import { LightControl, type LightControlProps } from '@/widgets/light-control/LightControl'

/** Props of every registered widget, keyed by its `type` string. */
export interface WidgetPropsMap {
  'light-control': LightControlProps
  'fan-control': FanControlProps
  'cover-control': CoverControlProps
  'door-button': DoorButtonProps
  'camera-preview': CameraPreviewProps
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
  'clock-weather': ClockWeather,
}

export const WIDGET_TYPES = [
  'light-control',
  'fan-control',
  'cover-control',
  'door-button',
  'camera-preview',
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
