import { describe, expect, it, vi } from 'vitest'

import type { Command, Target, Widget } from '@/api/generated'
import { bindWidget, type BindingContext } from '@/live/bindings'
import { applyAvailability, applySnapshot, EMPTY_DEVICE_DATA, selectEndpointView, type DeviceData } from '@/live/device-store'
import type { SendCommandOptions } from '@/live/live-client'
import { sampleSnapshot } from '@/live/test-fixtures'
import type { WidgetPropsMap } from '@/widgets/registry'

interface Sent {
  target: Target
  command: Command
  options: SendCommandOptions
}

function context() {
  const sent: Sent[] = []
  const opened: string[] = []
  const ctx: BindingContext = {
    send: (target, command, options) => {
      sent.push({ target, command, options })
      return Promise.resolve({ ok: true })
    },
    fallbackLabel: 'Unknown device',
    onOpened: (label) => opened.push(label),
  }
  return { ctx, sent, opened }
}

function bind<K extends keyof WidgetPropsMap>(type: K, widget: Omit<Widget, 'type' | 'col_span' | 'row_span'>, data?: DeviceData) {
  const state = data ?? applySnapshot(EMPTY_DEVICE_DATA, sampleSnapshot())
  const view = widget.target === undefined ? undefined : selectEndpointView(state, widget.target)
  const { ctx, sent, opened } = context()
  const spec = bindWidget({ type, col_span: 1, row_span: 1, ...widget }, view, ctx)
  if (spec.type !== type) throw new Error(`expected ${type}`)
  return { props: spec.props as WidgetPropsMap[K], sent, opened }
}

describe('bindWidget', () => {
  it('maps a dimmable white light onto light-control', () => {
    const { props, sent } = bind('light-control', { target: 'living_room_light/main', props: { label: 'Living Room' } })
    expect(props).toMatchObject({
      label: 'Living Room',
      icon: 'light',
      on: false,
      brightness: 80,
      colorTemperature: { kelvin: 3_000, minKelvin: 2_700, maxKelvin: 6_500 },
      pending: false,
      unavailable: false,
    })
    props.onToggle(true)
    props.onBrightnessChange?.(50)
    props.onColorTemperatureChange?.(4_000)
    expect(sent.map((s) => s.command)).toEqual([
      { capability: 'on_off', action: 'on' },
      { capability: 'level', action: 'set', level: 50 },
      { capability: 'color_temperature', action: 'set', kelvin: 4_000 },
    ])
    expect(sent.every((s) => s.target === 'living_room_light/main' && s.options.confirmed === false)).toBe(true)
  })

  it('only offers the controls the endpoint has', () => {
    const { props } = bind('light-control', { target: 'living_room_fan/light' })
    expect(props.label).toBe('Fan Light')
    expect(props.colorTemperature).toBeUndefined()
    expect(props.onColorTemperatureChange).toBeUndefined()
    expect(props.onBrightnessChange).toBeDefined()
  })

  it('maps a reversible fan', () => {
    const { props, sent } = bind('fan-control', { target: 'living_room_fan/fan' })
    expect(props).toMatchObject({ label: 'Ceiling Fan', speed: 1, speedSteps: 6, direction: 'forward', on: false })
    props.onSpeedChange(3)
    props.onDirectionChange?.('reverse')
    expect(sent.map((s) => s.command)).toEqual([
      { capability: 'fan', action: 'set_speed', speed: 3 },
      { capability: 'fan', action: 'set_direction', direction: 'reverse' },
    ])
  })

  it('maps a blind with position and stop, no confirmation', () => {
    const { props, sent } = bind('cover-control', { target: 'bedroom_blind/main' })
    expect(props).toMatchObject({ position: 100, motion: 'stopped', canStop: true, sensitive: false })
    props.onOpen()
    props.onPositionChange?.(30)
    expect(sent.map((s) => s.options.confirmed)).toEqual([false, false])
  })

  it('confirms garage doors before opening, never before closing', () => {
    const { props, sent } = bind('cover-control', { target: 'garage_door/main', props: { label: 'Garage' } })
    expect(props).toMatchObject({ icon: 'garage_door', sensitive: true, canStop: false, isOpen: false, position: undefined })
    expect(props.onStop).toBeUndefined()
    expect(props.onPositionChange).toBeUndefined()
    props.onOpen()
    props.onClose()
    expect(sent).toEqual([
      { target: 'garage_door/main', command: { capability: 'cover', action: 'open' }, options: { label: 'Garage', confirmed: true } },
      { target: 'garage_door/main', command: { capability: 'cover', action: 'close' }, options: { label: 'Garage', confirmed: false } },
    ])
  })

  it('opens a door strike as a confirmed command and reports success', async () => {
    const { props, sent, opened } = bind('door-button', { target: 'front_door_strike/main', props: { label: 'Front Door' } })
    expect(props.unavailable).toBe(false)
    props.onOpen()
    expect(sent).toEqual([
      {
        target: 'front_door_strike/main',
        command: { capability: 'trigger', action: 'fire' },
        options: { label: 'Front Door', confirmed: true },
      },
    ])
    await vi.waitFor(() => {
      expect(opened).toEqual(['Front Door'])
    })
  })

  it('shows the camera placeholder for a doorbell without a camera', () => {
    let data = applySnapshot(EMPTY_DEVICE_DATA, sampleSnapshot())
    data = applyAvailability(data, { device_id: 'front_doorbell', online: true })
    const { props } = bind('camera-preview', { target: 'front_doorbell/main' }, data)
    expect(props).toMatchObject({ label: 'Front Doorbell', icon: 'camera', unavailable: false })
  })

  it('renders offline or unknown devices as unavailable and never sends', () => {
    const offline = bind('camera-preview', { target: 'front_doorbell/main' })
    expect(offline.props.unavailable).toBe(true)

    const missing = bind('light-control', { target: 'attic/main', props: { label: 'Attic' } })
    expect(missing.props).toMatchObject({ label: 'Attic', unavailable: true })
    missing.props.onToggle(true)
    expect(missing.sent).toEqual([])

    const unnamed = bind('door-button', { target: 'attic/main' })
    expect(unnamed.props).toMatchObject({ label: 'Unknown device', unavailable: true })
  })

  it('shows no place name on the clock unless the dashboard gives one', () => {
    expect(bind('clock-weather', {}).props.label).toBeUndefined()
    expect(bind('clock-weather', { props: { label: 'Home' } }).props.label).toBe('Home')
  })

  it('keeps unknown widget types for the placeholder', () => {
    const { ctx } = context()
    expect(bindWidget({ type: 'thermostat', col_span: 1, row_span: 1, props: { x: 1 } }, undefined, ctx)).toEqual({
      type: 'thermostat',
      props: { x: 1 },
    })
  })
})
