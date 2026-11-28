import { describe, expect, it } from 'vitest'

import type { CapabilityKind, EndpointType } from '@/api/generated'
import {
  EXTRA_WIDGET_TYPES,
  getWidgetComponent,
  isWidgetType,
  pickWidgetType,
  WIDGET_TYPES,
  widgetMeta,
  widgetRegistry,
} from '@/widgets/registry'

describe('widget registry', () => {
  it('registers every listed widget type', () => {
    expect(Object.keys(widgetRegistry).sort()).toEqual([...WIDGET_TYPES].sort())
    for (const type of WIDGET_TYPES) {
      expect(typeof getWidgetComponent(type)).toBe('function')
    }
  })

  it('recognises only registered types', () => {
    expect(isWidgetType('light-control')).toBe(true)
    expect(isWidgetType('thermostat-control')).toBe(false)
    expect(isWidgetType('constructor')).toBe(false)
  })
})

describe('picking a widget for a thing in the home', () => {
  const pick = (type: EndpointType, ...capabilities: CapabilityKind[]) => pickWidgetType(type, capabilities)

  it('goes by endpoint type', () => {
    expect(pick('light', 'on_off', 'level', 'color_temperature')).toBe('light-control')
    expect(pick('outlet', 'on_off', 'power_meter')).toBe('light-control')
    expect(pick('switch', 'on_off')).toBe('light-control')
    expect(pick('fan', 'on_off', 'fan')).toBe('fan-control')
    expect(pick('window_covering', 'cover')).toBe('cover-control')
    expect(pick('door', 'trigger')).toBe('door-button')
    expect(pick('lock', 'lock')).toBe('door-button')
    expect(pick('camera', 'camera', 'occupancy')).toBe('camera-preview')
    expect(pick('doorbell', 'doorbell', 'occupancy')).toBe('camera-preview')
    expect(pick('ev_charger', 'ev_charger')).toBe('ev-charger')
    expect(pick('ev_charger', 'ev_charger', 'power_meter')).toBe('ev-charger')
  })

  it('uses capabilities to tell apart things of the same type', () => {
    // A garage door or gate that moves gets cover controls; one that opens
    // with a pulse (an electric strike or a relay) gets a door button.
    expect(pick('garage_door', 'cover')).toBe('cover-control')
    expect(pick('garage_door', 'trigger')).toBe('door-button')
    expect(pick('gate', 'cover')).toBe('cover-control')
    expect(pick('gate', 'trigger')).toBe('door-button')
    // A fan endpoint without fan controls, or a light without on/off: nothing fits.
    expect(pick('fan', 'on_off')).toBeNull()
    // A charger type without charger data (e.g. only a meter): nothing fits.
    expect(pick('ev_charger', 'power_meter')).toBeNull()
    expect(pick('light', 'level')).toBeNull()
  })

  it('offers nothing for things no widget can show yet', () => {
    expect(pick('sensor', 'temperature', 'humidity')).toBeNull()
    expect(pick('meter', 'power_meter')).toBeNull()
  })

  it('has metadata for every widget, and the clock needs no device', () => {
    expect(Object.keys(widgetMeta).sort()).toEqual([...WIDGET_TYPES].sort())
    expect(EXTRA_WIDGET_TYPES).toEqual(['clock-weather'])
  })
})
