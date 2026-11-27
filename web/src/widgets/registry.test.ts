import { describe, expect, it } from 'vitest'

import { getWidgetComponent, isWidgetType, WIDGET_TYPES, widgetRegistry } from '@/widgets/registry'

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
