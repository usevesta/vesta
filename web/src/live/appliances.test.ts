/**
 * Plugs and robot vacuums through the live layer: what the home sends is
 * accepted, tiles get the right props, a device that is only watched gets
 * no controls, and commands are predicted sensibly.
 */
import { describe, expect, it } from 'vitest'

import type { Command, Device, Target } from '@/api/generated'
import { decodeServerMessage } from '@/api/protocol'
import { bindWidget, type BindingContext } from '@/live/bindings'
import { applySnapshot, EMPTY_DEVICE_DATA, selectEndpointView } from '@/live/device-store'
import { predict } from '@/live/optimistic'
import { plugDevice, robotDevice } from '@/live/test-fixtures'
import type { WidgetPropsMap } from '@/widgets/registry'

function bind<K extends 'outlet-control' | 'robot-vacuum'>(type: K, device: Device) {
  const data = applySnapshot(EMPTY_DEVICE_DATA, { devices: [device] })
  const target: Target = `${device.id}/main`
  const sent: Command[] = []
  const ctx: BindingContext = {
    send: (_target, command) => {
      sent.push(command)
      return Promise.resolve({ ok: true })
    },
    fallbackLabel: 'Unknown device',
  }
  const spec = bindWidget({ type, target, col_span: 2, row_span: 1 }, selectEndpointView(data, target), ctx)
  if (spec.type !== type) throw new Error(`expected ${type}`)
  return { props: spec.props as WidgetPropsMap[K], sent }
}

describe('plugs', () => {
  it('arrive from the home with their meter and watch-only flag', () => {
    const decoded = decodeServerMessage({ type: 'snapshot', devices: [plugDevice({ readOnly: true })] })
    expect(decoded).toEqual({ type: 'snapshot', devices: [plugDevice({ readOnly: true })] })
  })

  it('get a switch and their readings', () => {
    const { props, sent } = bind('outlet-control', plugDevice())
    expect(props).toMatchObject({
      label: 'Hall Lamp',
      icon: 'outlet',
      on: true,
      powerW: 12.5,
      voltageV: 227.2,
      currentA: 0.112,
      unavailable: false,
    })
    expect(props.energyWh).toBeUndefined()
    props.onToggle?.(false)
    expect(sent).toEqual([{ capability: 'on_off', action: 'off' }])
  })

  it('that are only watched get no switch', () => {
    const { props, sent } = bind('outlet-control', plugDevice({ readOnly: true }))
    expect(props.onToggle).toBeUndefined()
    expect(props.on).toBe(true)
    expect(sent).toEqual([])
  })

  it('show as not responding when offline', () => {
    expect(bind('outlet-control', plugDevice({ online: false })).props.unavailable).toBe(true)
  })
})

describe('robot vacuums', () => {
  it('arrive from the home with their battery', () => {
    const decoded = decodeServerMessage({ type: 'snapshot', devices: [robotDevice()] })
    expect(decoded).toEqual({ type: 'snapshot', devices: [robotDevice()] })
    expect(
      decodeServerMessage({
        type: 'state',
        target: 'robot_vacuum/main',
        capability: 'robot_vacuum',
        state: { status: 'cleaning' },
      }),
    ).toMatchObject({ type: 'state', capability: 'robot_vacuum' })
  })

  it('get their status, battery, last clean and every control they have', () => {
    const { props, sent } = bind('robot-vacuum', robotDevice())
    expect(props).toMatchObject({
      label: 'Robot Vacuum',
      icon: 'robot_vacuum',
      status: 'docked',
      batteryPercent: 100,
      batteryLow: false,
      cleanedAreaM2: 22,
      cleaningDurationS: 1_800,
    })
    props.onStart?.()
    props.onPause?.()
    props.onReturnToDock?.()
    expect(sent).toEqual([
      { capability: 'robot_vacuum', action: 'start' },
      { capability: 'robot_vacuum', action: 'pause' },
      { capability: 'robot_vacuum', action: 'return_to_dock' },
    ])
  })

  it('that are only watched get no buttons', () => {
    const { props } = bind('robot-vacuum', robotDevice({ readOnly: true }))
    expect(props.onStart).toBeUndefined()
    expect(props.onPause).toBeUndefined()
    expect(props.onReturnToDock).toBeUndefined()
    expect(props.status).toBe('docked')
  })

  it('predict pausing and going home, but not how a start goes', () => {
    const robot = robotDevice().endpoints[0]
    expect(predict({ capability: 'robot_vacuum', action: 'pause' }, robot)).toEqual({
      predicted: { robot_vacuum: { status: 'paused' } },
      confirmsWith: 'robot_vacuum',
    })
    expect(predict({ capability: 'robot_vacuum', action: 'return_to_dock' }, robot).predicted).toEqual({
      robot_vacuum: { status: 'returning' },
    })
    expect(predict({ capability: 'robot_vacuum', action: 'start' }, robot)).toEqual({
      predicted: {},
      confirmsWith: 'robot_vacuum',
    })
  })
})
