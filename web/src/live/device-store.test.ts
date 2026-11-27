import { describe, expect, it } from 'vitest'

import type { Command, Target } from '@/api/generated'
import {
  acceptPending,
  addPending,
  applyAvailability,
  applySnapshot,
  applyStateChange,
  EMPTY_DEVICE_DATA,
  markUnsynced,
  removePending,
  selectEndpointView,
  type DeviceData,
} from '@/live/device-store'
import { predict } from '@/live/optimistic'
import { sampleSnapshot } from '@/live/test-fixtures'

const FAN_LIGHT = 'living_room_fan/light'

function loaded(): DeviceData {
  return applySnapshot(EMPTY_DEVICE_DATA, sampleSnapshot())
}

function send(data: DeviceData, requestId: string, target: Target, command: Command): DeviceData {
  const { predicted, confirmsWith } = predict(command, selectEndpointView(data, target)?.endpoint)
  return addPending(data, { requestId, target, predicted, confirmsWith, accepted: false, observed: false, confirmed: false })
}

describe('device store', () => {
  it('indexes devices by id and endpoints by target', () => {
    const data = loaded()
    expect(data.synced).toBe(true)
    expect(Object.keys(data.devices)).toContain('living_room_fan')
    expect(data.endpoints[FAN_LIGHT]?.name).toBe('Fan Light')
    expect(data.endpoints['living_room_fan/fan']?.type).toBe('fan')
    expect(selectEndpointView(data, FAN_LIGHT)).toMatchObject({ available: true, pending: false })
  })

  it('applies state and availability updates', () => {
    let data = loaded()
    data = applyStateChange(data, { target: FAN_LIGHT, capability: 'on_off', state: { on: true } })
    expect(data.endpoints[FAN_LIGHT]?.capabilities.on_off?.state).toEqual({ on: true })

    data = applyAvailability(data, { device_id: 'living_room_fan', online: false })
    expect(selectEndpointView(data, FAN_LIGHT)?.available).toBe(false)
    // Offline devices keep their last state.
    expect(data.endpoints[FAN_LIGHT]?.capabilities.on_off?.state).toEqual({ on: true })
  })

  it('ignores updates for unknown targets and capabilities', () => {
    const data = loaded()
    expect(applyStateChange(data, { target: 'nowhere/main', capability: 'on_off', state: { on: true } })).toBe(data)
    expect(applyStateChange(data, { target: FAN_LIGHT, capability: 'fan', state: { speed: 2, direction: 'forward' } })).toBe(
      data,
    )
    expect(applyAvailability(data, { device_id: 'nowhere', online: false })).toBe(data)
  })

  it('treats every snapshot as a full replacement', () => {
    let data = applyStateChange(loaded(), { target: FAN_LIGHT, capability: 'on_off', state: { on: true } })
    data = markUnsynced(data)
    expect(selectEndpointView(data, FAN_LIGHT)?.available).toBe(false)
    data = applySnapshot(data, { devices: [] })
    expect(data.synced).toBe(true)
    expect(data.endpoints).toEqual({})
  })

  it('shows a prediction as pending until the matching state arrives', () => {
    let data = send(loaded(), 'c-1', FAN_LIGHT, { capability: 'on_off', action: 'on' })
    expect(selectEndpointView(data, FAN_LIGHT)).toMatchObject({
      pending: true,
      endpoint: { capabilities: { on_off: { state: { on: true } } } },
    })
    // The confirmed state is untouched.
    expect(data.endpoints[FAN_LIGHT]?.capabilities.on_off?.state).toEqual({ on: false })

    // A state that does not match yet (e.g. an older one) keeps it pending.
    data = applyStateChange(data, { target: FAN_LIGHT, capability: 'on_off', state: { on: false } })
    expect(selectEndpointView(data, FAN_LIGHT)?.pending).toBe(true)

    data = applyStateChange(data, { target: FAN_LIGHT, capability: 'on_off', state: { on: true } })
    expect(selectEndpointView(data, FAN_LIGHT)?.pending).toBe(false)
    expect(data.pending).toEqual({})
  })

  it('resolves on ok plus any state when the device adjusted the value', () => {
    const target = 'living_room_light/main'
    let data = send(loaded(), 'c-2', target, { capability: 'color_temperature', action: 'set', kelvin: 2_000 })
    // The device clamps to its range: the state never matches the prediction.
    data = applyStateChange(data, { target, capability: 'color_temperature', state: { kelvin: 2_700 } })
    expect(selectEndpointView(data, target)?.pending).toBe(true)
    data = acceptPending(data, 'c-2')
    expect(selectEndpointView(data, target)).toMatchObject({
      pending: false,
      endpoint: { capabilities: { color_temperature: { state: { kelvin: 2_700 } } } },
    })
  })

  it('waits for every predicted capability (level.set also switches on)', () => {
    let data = send(loaded(), 'c-3', FAN_LIGHT, { capability: 'level', action: 'set', level: 40 })
    expect(selectEndpointView(data, FAN_LIGHT)?.endpoint.capabilities).toMatchObject({
      on_off: { state: { on: true } },
      level: { state: { level: 40 } },
    })
    data = applyStateChange(data, { target: FAN_LIGHT, capability: 'on_off', state: { on: true } })
    expect(selectEndpointView(data, FAN_LIGHT)?.pending).toBe(true)
    data = applyStateChange(data, { target: FAN_LIGHT, capability: 'level', state: { level: 40 } })
    expect(selectEndpointView(data, FAN_LIGHT)?.pending).toBe(false)
  })

  it('resolves commands without state (door strike) on ok', () => {
    const target = 'front_door_strike/main'
    let data = send(loaded(), 'c-4', target, { capability: 'trigger', action: 'fire' })
    expect(selectEndpointView(data, target)?.pending).toBe(true)
    data = acceptPending(data, 'c-4')
    expect(selectEndpointView(data, target)?.pending).toBe(false)
  })

  it('reverts when a command is removed', () => {
    let data = send(loaded(), 'c-5', 'living_room_fan/fan', { capability: 'fan', action: 'set_speed', speed: 4 })
    expect(selectEndpointView(data, 'living_room_fan/fan')?.endpoint.capabilities.fan?.state?.speed).toBe(4)
    data = removePending(data, 'c-5')
    expect(selectEndpointView(data, 'living_room_fan/fan')).toMatchObject({
      pending: false,
      endpoint: { capabilities: { fan: { state: { speed: 1 } }, on_off: { state: { on: false } } } },
    })
  })

  it('stacks quick taps: the newest prediction wins until each is confirmed', () => {
    const target = 'living_room_fan/fan'
    let data = send(loaded(), 'c-6', target, { capability: 'fan', action: 'set_speed', speed: 2 })
    data = send(data, 'c-7', target, { capability: 'fan', action: 'set_speed', speed: 3 })
    expect(selectEndpointView(data, target)?.endpoint.capabilities.fan?.state?.speed).toBe(3)
    // Picking a speed starts the fan.
    data = applyStateChange(data, { target, capability: 'on_off', state: { on: true } })
    data = applyStateChange(data, { target, capability: 'fan', state: { speed: 2, direction: 'forward' } })
    expect(selectEndpointView(data, target)).toMatchObject({
      pending: true,
      endpoint: { capabilities: { fan: { state: { speed: 3 } } } },
    })
    data = applyStateChange(data, { target, capability: 'fan', state: { speed: 3, direction: 'forward' } })
    expect(selectEndpointView(data, target)?.pending).toBe(false)
  })

  it('settles pending commands from a snapshot after a reconnect', () => {
    let data = send(loaded(), 'c-8', FAN_LIGHT, { capability: 'on_off', action: 'on' })
    data = markUnsynced(data)
    const snapshot = sampleSnapshot()
    const light = snapshot.devices[1]?.endpoints[1]
    if (light?.capabilities.on_off === undefined) throw new Error('fixture changed')
    light.capabilities.on_off.state = { on: true }
    data = applySnapshot(data, snapshot)
    expect(data.pending).toEqual({})
  })
})
