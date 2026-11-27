import { describe, expect, it } from 'vitest'

import { decodeDashboard, decodeServerMessage } from '@/api/protocol'
import { makeTarget, parseTarget } from '@/api/target'
import { sampleDevices } from '@/live/test-fixtures'

describe('decodeServerMessage', () => {
  it('accepts every message type the home sends', () => {
    const snapshot = { type: 'snapshot', devices: sampleDevices() }
    expect(decodeServerMessage(snapshot)).toEqual(snapshot)

    const state = { type: 'state', target: 'living_room_fan/light', capability: 'level', state: { level: 80 } }
    expect(decodeServerMessage(state)).toEqual(state)

    const event = { type: 'event', target: 'front_doorbell/main', event: 'doorbell.pressed', at: '2026-01-01T12:00:00Z' }
    expect(decodeServerMessage(event)).toEqual(event)

    const availability = { type: 'availability', device_id: 'living_room_fan', online: false }
    expect(decodeServerMessage(availability)).toEqual(availability)

    expect(decodeServerMessage({ type: 'command_result', request_id: 'c-1', ok: true })).toEqual({
      type: 'command_result',
      request_id: 'c-1',
      ok: true,
    })
    expect(
      decodeServerMessage({
        type: 'command_result',
        request_id: 'c-2',
        ok: false,
        error: { code: 'rejected', reason: 'confirmation_required' },
      }),
    ).toEqual({
      type: 'command_result',
      request_id: 'c-2',
      ok: false,
      error: { code: 'rejected', reason: 'confirmation_required' },
    })
  })

  it('drops unknown or malformed messages', () => {
    for (const bad of [
      null,
      'snapshot',
      [],
      {},
      { type: 'hello' },
      { type: 'snapshot' },
      { type: 'state', target: 'a/main', capability: 'teleport', state: {} },
      { type: 'state', target: 'a/main', capability: 'trigger', state: {} },
      { type: 'state', target: 'a/main', capability: 'level', state: 80 },
      { type: 'event', target: 'a/main', event: 'doorbell.exploded', at: 'now' },
      { type: 'availability', device_id: 'a', online: 'yes' },
      { type: 'command_result', ok: true },
    ]) {
      expect(decodeServerMessage(bad), JSON.stringify(bad)).toBeNull()
    }
  })

  it('keeps what it understands from a newer home', () => {
    const decoded = decodeServerMessage({
      type: 'snapshot',
      devices: [
        {
          id: 'kitchen',
          name: 'Kitchen',
          online: true,
          endpoints: [
            { id: 'main', name: 'Kitchen Light', type: 'light', capabilities: { on_off: { state: { on: true } }, sparkle: {} } },
            { id: 'robot', name: 'Robot', type: 'robot_arm', capabilities: {} },
          ],
        },
        { id: 'broken' },
      ],
    })
    expect(decoded).toEqual({
      type: 'snapshot',
      devices: [
        {
          id: 'kitchen',
          name: 'Kitchen',
          online: true,
          endpoints: [{ id: 'main', name: 'Kitchen Light', type: 'light', capabilities: { on_off: { state: { on: true } } } }],
        },
      ],
    })
  })

  it('rejects capabilities that miss their features', () => {
    const decoded = decodeServerMessage({
      type: 'snapshot',
      devices: [
        {
          id: 'fan',
          name: 'Fan',
          online: true,
          endpoints: [{ id: 'main', name: 'Fan', type: 'fan', capabilities: { fan: { state: { speed: 1 } } } }],
        },
      ],
    })
    expect(decoded).toEqual({ type: 'snapshot', devices: [{ id: 'fan', name: 'Fan', online: true, endpoints: [] }] })
  })

  it('turns unknown failures into a generic device error and never keeps raw messages', () => {
    expect(
      decodeServerMessage({ type: 'command_result', request_id: 'c-3', ok: false, error: { code: 'meltdown' } }),
    ).toEqual({ type: 'command_result', request_id: 'c-3', ok: false, error: { code: 'device' } })
    expect(
      decodeServerMessage({
        type: 'command_result',
        request_id: 'c-4',
        ok: false,
        error: { code: 'device', message: 'relay 192.0.2.1 refused' },
      }),
    ).toEqual({ type: 'command_result', request_id: 'c-4', ok: false, error: { code: 'device' } })
  })
})

describe('decodeDashboard', () => {
  const dashboard = {
    schema_version: 1,
    theme: { preset: 'glass-dark', auto_dark_mode: true, touch_scale: 'large' },
    kiosk: {
      screensaver_timeout_sec: 120,
      wake_on: [
        { target: 'front_doorbell/main', event: 'doorbell.pressed' },
        { target: 'front_doorbell/main', event: 'occupancy.detected' },
      ],
    },
    doorbell_overlay: {
      doorbell_target: 'front_doorbell/main',
      camera_target: 'front_doorbell/main',
      open_target: 'front_door_strike/main',
      auto_dismiss_sec: 45,
    },
    pages: [
      {
        id: 'home',
        title: 'Home',
        icon: 'home',
        grid_columns: 4,
        widgets: [
          { type: 'clock-weather', col_span: 2, row_span: 1 },
          { type: 'door-button', target: 'front_door_strike/main', col_span: 2, row_span: 1, props: { label: 'Front Door' } },
          { type: 'camera-preview', target: 'front_doorbell/main', col_span: 2, row_span: 2, props: { live_webrtc: true } },
        ],
      },
    ],
  }

  it('accepts a dashboard as the home sends it', () => {
    expect(decodeDashboard(dashboard)).toEqual(dashboard)
  })

  it('skips wake events this version does not know', () => {
    const decoded = decodeDashboard({
      ...dashboard,
      kiosk: { screensaver_timeout_sec: 60, wake_on: [{ target: 'a/main', event: 'garden.gnome' }] },
    })
    expect(decoded?.kiosk).toEqual({ screensaver_timeout_sec: 60, wake_on: [] })
  })

  it('rejects broken dashboards', () => {
    expect(decodeDashboard(null)).toBeNull()
    expect(decodeDashboard({ ...dashboard, pages: 'home' })).toBeNull()
    expect(decodeDashboard({ ...dashboard, kiosk: {} })).toBeNull()
    expect(decodeDashboard({ ...dashboard, pages: [{ id: 'x', title: 'X', grid_columns: 2, widgets: [{}] }] })).toBeNull()
    expect(decodeDashboard({ ...dashboard, doorbell_overlay: { auto_dismiss_sec: 5 } })).toBeNull()
  })
})

describe('targets', () => {
  it('round-trips device and endpoint ids', () => {
    expect(makeTarget('living_room_fan', 'light')).toBe('living_room_fan/light')
    expect(parseTarget('living_room_fan/light')).toEqual({ deviceId: 'living_room_fan', endpointId: 'light' })
  })

  it('rejects malformed targets', () => {
    for (const bad of ['', 'fan', '/light', 'fan/', 'a/b/c']) expect(parseTarget(bad), bad).toBeNull()
  })
})
