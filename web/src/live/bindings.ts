/**
 * Dashboard widget + live endpoint → presentational widget props.
 *
 * The dashboard says *which* widget shows *which* target; the endpoint's
 * capabilities and features decide which controls appear (a light without
 * `level` gets no brightness slider). Pure functions: commands go out
 * through `BindingContext.send`, so this is testable without a socket.
 */
import type { Command, Target, Widget } from '@/api/generated'
import { isIconName, type IconName } from '@/lib/icons'
import type { EndpointView } from '@/live/device-store'
import type { CommandOutcome, SendCommandOptions } from '@/live/live-client'
import { guardsAccess, openCommand } from '@/live/sensitive'
import { chargerCurrentRange, type EnergyReadings } from '@/widgets/ev-charger/energy'
import { isWidgetType, type AnyWidgetSpec, type WidgetSpec, type WidgetType } from '@/widgets/registry'

export interface BindingContext {
  send: (target: Target, command: Command, options: SendCommandOptions) => Promise<CommandOutcome>
  /** Name to show when neither the dashboard nor the device gives one. */
  fallbackLabel: string
  /** A door, gate or lock was opened (the app shows a short confirmation). */
  onOpened?: (label: string) => void
}

function stringProp(widget: Widget, key: string): string | undefined {
  const value = widget.props?.[key]
  return typeof value === 'string' && value.trim() !== '' ? value : undefined
}

/** `props.icon` if it is a known icon, else the endpoint type's icon. */
function iconFor(widget: Widget, view: EndpointView | undefined, fallback?: IconName): IconName | undefined {
  const custom = stringProp(widget, 'icon')
  if (custom !== undefined && isIconName(custom)) return custom
  if (fallback !== undefined) return fallback
  const type = view?.endpoint.type
  return type !== undefined && isIconName(type) ? type : undefined
}

function labelFor(widget: Widget, view: EndpointView | undefined, ctx: BindingContext): string {
  return stringProp(widget, 'label') ?? view?.endpoint.name ?? ctx.fallbackLabel
}

/**
 * The home's meters next to a charger (endpoints of the same device), by
 * role. A meter without a reading yet is left out.
 */
export function meterReadings(related: readonly EndpointView[]): EnergyReadings {
  const readings: { -readonly [K in keyof EnergyReadings]: EnergyReadings[K] } = {}
  for (const other of related) {
    const meter = other.endpoint.capabilities.power_meter
    const state = meter?.state
    if (meter === undefined || state === undefined) continue
    switch (meter.features.role) {
      case 'grid':
        readings.gridW = state.power_w
        if (state.limit_w !== undefined) readings.limitW = state.limit_w
        break
      case 'house':
        readings.houseW = state.power_w
        break
      case 'solar':
        readings.solarW = state.power_w
        break
      case 'battery':
        readings.batteryW = state.power_w
        break
      case 'load':
      case 'ev':
        // Single circuits are not part of the home's balance.
        break
    }
  }
  return readings
}

function bindKnown(
  type: WidgetType,
  widget: Widget,
  view: EndpointView | undefined,
  ctx: BindingContext,
  related: readonly EndpointView[],
): WidgetSpec {
  const label = labelFor(widget, view, ctx)
  const capabilities = view?.endpoint.capabilities
  const status = { pending: view?.pending ?? false, unavailable: view?.available !== true }
  const target = view?.target
  // Callbacks hand the outcome back, so the control that sent it is busy
  // until the home answered (see `useCommand`).
  const run = (command: Command, confirmed = false): Promise<CommandOutcome> | undefined => {
    if (target === undefined) return undefined
    return ctx.send(target, command, { label, confirmed })
  }

  switch (type) {
    case 'light-control': {
      const onOff = capabilities?.on_off
      const level = capabilities?.level
      const colorTemperature = capabilities?.color_temperature
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || onOff === undefined,
          label,
          icon: iconFor(widget, view),
          on: onOff?.state?.on ?? false,
          onToggle: (on) => run({ capability: 'on_off', action: on ? 'on' : 'off' }),
          brightness: level?.state?.level,
          onBrightnessChange:
            level === undefined
              ? undefined
              : (value) => run({ capability: 'level', action: 'set', level: value }),
          colorTemperature:
            colorTemperature?.state === undefined
              ? undefined
              : {
                  kelvin: colorTemperature.state.kelvin,
                  minKelvin: colorTemperature.features.min_kelvin,
                  maxKelvin: colorTemperature.features.max_kelvin,
                },
          onColorTemperatureChange:
            colorTemperature === undefined
              ? undefined
              : (kelvin) => run({ capability: 'color_temperature', action: 'set', kelvin }),
        },
      }
    }

    case 'outlet-control': {
      const onOff = capabilities?.on_off
      const meter = capabilities?.power_meter?.state
      // A plug that is only watched gets no switch (the home would refuse it).
      const switchable = onOff !== undefined && onOff.features?.read_only !== true
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || onOff === undefined,
          label,
          icon: iconFor(widget, view),
          on: onOff?.state?.on,
          onToggle: switchable
            ? (on) => run({ capability: 'on_off', action: on ? 'on' : 'off' })
            : undefined,
          powerW: meter?.power_w,
          energyWh: meter?.energy_wh,
          voltageV: meter?.voltage_v,
          currentA: meter?.current_a,
        },
      }
    }

    case 'robot-vacuum': {
      const robot = capabilities?.robot_vacuum
      const features = robot?.features
      const battery = capabilities?.battery?.state
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || robot === undefined,
          label,
          icon: iconFor(widget, view, 'robot_vacuum'),
          status: robot?.state?.status,
          batteryPercent: battery?.percent,
          batteryLow: battery?.low,
          cleanedAreaM2: robot?.state?.cleaned_area_m2,
          cleaningDurationS: robot?.state?.cleaning_duration_s,
          onStart:
            features?.start === true
              ? () => run({ capability: 'robot_vacuum', action: 'start' })
              : undefined,
          onPause:
            features?.pause === true
              ? () => run({ capability: 'robot_vacuum', action: 'pause' })
              : undefined,
          onReturnToDock:
            features?.return_to_dock === true
              ? () => run({ capability: 'robot_vacuum', action: 'return_to_dock' })
              : undefined,
        },
      }
    }

    case 'fan-control': {
      const onOff = capabilities?.on_off
      const fan = capabilities?.fan
      const reversible = fan?.features.reversible === true
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || fan === undefined || onOff === undefined,
          label,
          icon: iconFor(widget, view),
          on: onOff?.state?.on ?? false,
          onToggle: (on) => run({ capability: 'on_off', action: on ? 'on' : 'off' }),
          speed: fan?.state?.speed ?? 1,
          speedSteps: fan?.features.speed_steps ?? 1,
          onSpeedChange: (speed) => run({ capability: 'fan', action: 'set_speed', speed }),
          direction: reversible ? (fan.state?.direction ?? 'forward') : undefined,
          onDirectionChange: reversible
            ? (direction) => run({ capability: 'fan', action: 'set_direction', direction })
            : undefined,
        },
      }
    }

    case 'cover-control': {
      const cover = capabilities?.cover
      // Garage doors and gates: the widget asks first, then we send `confirmed`.
      const sensitive = view !== undefined && guardsAccess(view.endpoint.type)
      const canStop = cover?.features.stop === true
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || cover === undefined,
          label,
          icon: iconFor(widget, view),
          sensitive,
          position: cover?.features.position === true ? cover.state?.position : undefined,
          isOpen: cover?.state?.open,
          motion: cover?.state?.motion,
          canStop,
          onOpen: () => run({ capability: 'cover', action: 'open' }, sensitive),
          onClose: () => run({ capability: 'cover', action: 'close' }),
          onStop: canStop
            ? () => run({ capability: 'cover', action: 'stop' })
            : undefined,
          onPositionChange:
            cover?.features.position === true
              ? (position) => run({ capability: 'cover', action: 'set_position', position }, sensitive)
              : undefined,
        },
      }
    }

    case 'door-button': {
      const command = view === undefined ? null : openCommand(view.endpoint)
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || command === null,
          label,
          icon: iconFor(widget, view, view?.endpoint.type === 'gate' ? 'gate' : undefined),
          onOpen: () => {
            // DoorButton only calls this after the person confirmed.
            if (target === undefined || command === null) return undefined
            return ctx.send(target, command, { label, confirmed: true }).then((outcome) => {
              if (outcome.ok) ctx.onOpened?.(label)
              return outcome
            })
          },
        },
      }
    }

    case 'camera-preview':
      // Live video is not wired yet: every camera shows the "not available
      // yet" placeholder, including targets without a `camera` capability
      // (e.g. a doorbell). Only an offline or missing device is unavailable.
      return {
        type,
        props: {
          ...status,
          pending: false,
          label,
          icon: iconFor(widget, view, 'camera'),
        },
      }

    case 'ev-charger': {
      const charger = capabilities?.ev_charger
      const state = charger?.state
      const features = charger?.features
      const send = run
      const modes = features?.modes ?? []
      return {
        type,
        props: {
          ...status,
          unavailable: status.unavailable || charger === undefined,
          label,
          icon: iconFor(widget, view, 'ev_charger'),
          rows: widget.row_span,
          status: state?.status,
          waitingFor: state?.waiting_for,
          powerW: state?.power_w,
          sessionEnergyWh: state?.session_energy_wh,
          sessionDurationS: state?.session_duration_s,
          currentA: state?.current_a,
          measuredA: state?.phase_current_a,
          currentRange: features === undefined ? undefined : chargerCurrentRange(features, state),
          autoCurrent: state?.auto_current,
          mode: state?.mode,
          modes,
          locked: state?.locked,
          scheduleEnabled: state?.schedule_enabled,
          readings: meterReadings(related),
          onPause:
            features?.pause === true
              ? () => send({ capability: 'ev_charger', action: 'pause' })
              : undefined,
          onResume:
            features?.pause === true
              ? () => send({ capability: 'ev_charger', action: 'resume' })
              : undefined,
          onCurrentChange:
            features?.adjustable_current === true
              ? (amps) => send({ capability: 'ev_charger', action: 'set_current', amps })
              : undefined,
          onModeChange:
            modes.length > 0
              ? (mode) => send({ capability: 'ev_charger', action: 'set_mode', mode })
              : undefined,
          // The widget asks before unlocking; only then is it sent as confirmed.
          onLockChange:
            features?.lock === true
              ? (locked) => send({ capability: 'ev_charger', action: locked ? 'lock' : 'unlock' }, !locked)
              : undefined,
          onScheduleChange:
            features?.schedule === true
              ? (enabled) => send({ capability: 'ev_charger', action: 'set_schedule', enabled })
              : undefined,
        },
      }
    }

    case 'clock-weather':
      // Shows no place name unless the dashboard gives one.
      return { type, props: { label: stringProp(widget, 'label') } }
  }
}

/**
 * Build the registry spec for one dashboard widget. Unknown types keep
 * their type. `related` are the other endpoints of the same device (a
 * charger's meters).
 */
export function bindWidget(
  widget: Widget,
  view: EndpointView | undefined,
  ctx: BindingContext,
  related: readonly EndpointView[] = [],
): WidgetSpec | AnyWidgetSpec {
  if (!isWidgetType(widget.type)) return { type: widget.type, props: widget.props }
  return bindKnown(widget.type, widget, view, ctx, related)
}
