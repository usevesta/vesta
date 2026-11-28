/**
 * The charger tile in the live layer: binding (with the device's meters),
 * what its commands predict, and which of them need a confirmation.
 */
import { describe, expect, it } from 'vitest'

import type { Command, Device, Target, Widget } from '@/api/generated'
import { bindWidget, meterReadings, type BindingContext } from '@/live/bindings'
import {
  applySnapshot,
  EMPTY_DEVICE_DATA,
  selectEndpointView,
  selectRelatedEndpointViews,
  type DeviceData,
} from '@/live/device-store'
import type { SendCommandOptions } from '@/live/live-client'
import { applyPrediction, predict } from '@/live/optimistic'
import { isSensitiveCommand } from '@/live/sensitive'
import { chargerDevice, sampleDevices } from '@/live/test-fixtures'
import type { WidgetPropsMap } from '@/widgets/registry'

const CHARGER: Target = 'car_charger/charger'

function home(devices: Device[] = [...sampleDevices(), chargerDevice()]): DeviceData {
  return applySnapshot(EMPTY_DEVICE_DATA, { devices })
}

function bindCharger(data: DeviceData, widget: Partial<Widget> = {}) {
  const sent: { target: Target; command: Command; options: SendCommandOptions }[] = []
  const ctx: BindingContext = {
    send: (target, command, options) => {
      sent.push({ target, command, options })
      return Promise.resolve({ ok: true })
    },
    fallbackLabel: 'Unknown device',
  }
  const spec = bindWidget(
    { type: 'ev-charger', target: CHARGER, col_span: 2, row_span: 2, ...widget },
    selectEndpointView(data, CHARGER),
    ctx,
    selectRelatedEndpointViews(data, CHARGER),
  )
  if (spec.type !== 'ev-charger') throw new Error('expected the charger tile')
  return { props: spec.props as WidgetPropsMap['ev-charger'], sent }
}

describe('the charger tile', () => {
  it('finds the meters next to the charger, and only those', () => {
    const data = home()
    const related = selectRelatedEndpointViews(data, CHARGER)
    expect(related.map((view) => view.target)).toEqual(['car_charger/grid', 'car_charger/house', 'car_charger/solar'])
    expect(meterReadings(related)).toEqual({ gridW: 5_300, limitW: 5_750, houseW: 6_100, solarW: 800 })
    expect(selectRelatedEndpointViews(data, 'living_room_light/main')).toEqual([])
    expect(selectRelatedEndpointViews(data, 'not a target')).toEqual([])
  })

  it('shows the charger in plain values', () => {
    const { props } = bindCharger(home(), { props: { label: 'Garage charger' } })
    expect(props).toMatchObject({
      label: 'Garage charger',
      icon: 'ev_charger',
      rows: 2,
      status: 'charging',
      powerW: 5_092,
      sessionEnergyWh: 4_890,
      sessionDurationS: 3_841,
      currentA: 23,
      measuredA: [22.94, 0, 0],
      currentRange: { min: 6, max: 30 },
      autoCurrent: false,
      mode: 'manual',
      modes: ['manual', 'solar', 'solar_grid'],
      locked: false,
      scheduleEnabled: true,
      readings: { gridW: 5_300, limitW: 5_750, houseW: 6_100, solarW: 800 },
      pending: false,
      unavailable: false,
    })
  })

  it('sends each control as its command; unlocking only as confirmed', () => {
    const { props, sent } = bindCharger(home())
    props.onPause?.()
    props.onResume?.()
    props.onCurrentChange?.(16)
    props.onModeChange?.('solar')
    props.onLockChange?.(true)
    props.onLockChange?.(false)
    props.onScheduleChange?.(false)
    expect(sent.map((s) => s.command)).toEqual([
      { capability: 'ev_charger', action: 'pause' },
      { capability: 'ev_charger', action: 'resume' },
      { capability: 'ev_charger', action: 'set_current', amps: 16 },
      { capability: 'ev_charger', action: 'set_mode', mode: 'solar' },
      { capability: 'ev_charger', action: 'lock' },
      { capability: 'ev_charger', action: 'unlock' },
      { capability: 'ev_charger', action: 'set_schedule', enabled: false },
    ])
    expect(sent.map((s) => s.options.confirmed)).toEqual([false, false, false, false, false, true, false])
    expect(sent.every((s) => s.target === CHARGER)).toBe(true)
  })

  it('reads the hours from the charger only when it can, and quietly', () => {
    const { props, sent } = bindCharger(home())
    expect(props.scheduleFeatures?.refreshable).toBe(true)
    props.onRefreshChargingSchedule?.()
    expect(sent).toEqual([
      {
        target: CHARGER,
        command: { capability: 'ev_charger', action: 'refresh_charging_schedule' },
        // The editor says itself when it did not work.
        options: { label: 'Car Charger', confirmed: false, quiet: true },
      },
    ])
    expect(predict({ capability: 'ev_charger', action: 'refresh_charging_schedule' }, chargerDevice().endpoints[0])).toEqual({
      predicted: {},
      confirmsWith: null,
    })

    const reporting = chargerDevice()
    const schedule = reporting.endpoints[0]?.capabilities.ev_charger?.features.charging_schedule
    if (schedule === undefined) throw new Error('fixture changed')
    schedule.refreshable = false
    schedule.reports_windows = true
    expect(bindCharger(home([reporting])).props.onRefreshChargingSchedule).toBeUndefined()
  })

  it('offers no control the charger does not have (monitoring only)', () => {
    const device = chargerDevice()
    const [charger] = device.endpoints
    const capability = charger?.capabilities.ev_charger
    if (capability === undefined) throw new Error('no charger')
    capability.features = {
      ...capability.features,
      modes: [],
      pause: false,
      adjustable_current: false,
      lock: false,
      schedule: false,
    }
    const { props } = bindCharger(home([device]))
    expect(props.onPause).toBeUndefined()
    expect(props.onResume).toBeUndefined()
    expect(props.onCurrentChange).toBeUndefined()
    expect(props.onModeChange).toBeUndefined()
    expect(props.onLockChange).toBeUndefined()
    expect(props.onScheduleChange).toBeUndefined()
    // Still shows what it knows.
    expect(props.locked).toBe(false)
    expect(props.scheduleEnabled).toBe(true)
  })

  it('is unavailable while the charger does not respond', () => {
    const { props } = bindCharger(home([chargerDevice({ online: false })]))
    expect(props.unavailable).toBe(true)
    const missing = bindWidget(
      { type: 'ev-charger', target: 'car_charger/charger', col_span: 1, row_span: 1 },
      undefined,
      { send: () => Promise.resolve({ ok: true }), fallbackLabel: 'Unknown device' },
    )
    expect(missing.props).toMatchObject({ unavailable: true, label: 'Unknown device', readings: {} })
  })
})

describe('charger commands', () => {
  const endpoint = chargerDevice().endpoints[0]

  it('show the change at once', () => {
    const ev = (command: Extract<Command, { capability: 'ev_charger' }>) => predict(command, endpoint)
    expect(ev({ capability: 'ev_charger', action: 'pause' })).toEqual({
      predicted: { ev_charger: { status: 'paused', power_w: 0 } },
      confirmsWith: 'ev_charger',
    })
    // The charger decides what resuming leads to.
    expect(ev({ capability: 'ev_charger', action: 'resume' })).toEqual({ predicted: {}, confirmsWith: 'ev_charger' })
    expect(ev({ capability: 'ev_charger', action: 'set_current', amps: 16 }).predicted).toEqual({
      ev_charger: { current_a: 16 },
    })
    expect(ev({ capability: 'ev_charger', action: 'set_mode', mode: 'solar' }).predicted).toEqual({
      ev_charger: { mode: 'solar' },
    })
    expect(ev({ capability: 'ev_charger', action: 'unlock' }).predicted).toEqual({ ev_charger: { locked: false } })
    expect(ev({ capability: 'ev_charger', action: 'set_schedule', enabled: false }).predicted).toEqual({
      ev_charger: { schedule_enabled: false },
    })

    const shown = applyPrediction(endpoint?.capabilities ?? {}, { ev_charger: { status: 'paused', power_w: 0 } })
    expect(shown.ev_charger?.state).toMatchObject({ status: 'paused', power_w: 0, current_a: 23 })
  })

  it('need a confirmation only to unlock', () => {
    const actions: Extract<Command, { capability: 'ev_charger' }>[] = [
      { capability: 'ev_charger', action: 'pause' },
      { capability: 'ev_charger', action: 'resume' },
      { capability: 'ev_charger', action: 'set_current', amps: 10 },
      { capability: 'ev_charger', action: 'set_mode', mode: 'manual' },
      { capability: 'ev_charger', action: 'lock' },
      { capability: 'ev_charger', action: 'set_schedule', enabled: true },
    ]
    for (const command of actions) expect(isSensitiveCommand('ev_charger', command), command.action).toBe(false)
    expect(isSensitiveCommand('ev_charger', { capability: 'ev_charger', action: 'unlock' })).toBe(true)
  })
})
