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

function bindKnown(type: WidgetType, widget: Widget, view: EndpointView | undefined, ctx: BindingContext): WidgetSpec {
  const label = labelFor(widget, view, ctx)
  const capabilities = view?.endpoint.capabilities
  const status = { pending: view?.pending ?? false, unavailable: view?.available !== true }
  const target = view?.target
  const run = (command: Command, confirmed = false) => {
    if (target === undefined) return
    void ctx.send(target, command, { label, confirmed })
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
          onToggle: (on) => {
            run({ capability: 'on_off', action: on ? 'on' : 'off' })
          },
          brightness: level?.state?.level,
          onBrightnessChange:
            level === undefined
              ? undefined
              : (value) => {
                  run({ capability: 'level', action: 'set', level: value })
                },
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
              : (kelvin) => {
                  run({ capability: 'color_temperature', action: 'set', kelvin })
                },
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
          onToggle: (on) => {
            run({ capability: 'on_off', action: on ? 'on' : 'off' })
          },
          speed: fan?.state?.speed ?? 1,
          speedSteps: fan?.features.speed_steps ?? 1,
          onSpeedChange: (speed) => {
            run({ capability: 'fan', action: 'set_speed', speed })
          },
          direction: reversible ? (fan.state?.direction ?? 'forward') : undefined,
          onDirectionChange: reversible
            ? (direction) => {
                run({ capability: 'fan', action: 'set_direction', direction })
              }
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
          onOpen: () => {
            run({ capability: 'cover', action: 'open' }, sensitive)
          },
          onClose: () => {
            run({ capability: 'cover', action: 'close' })
          },
          onStop: canStop
            ? () => {
                run({ capability: 'cover', action: 'stop' })
              }
            : undefined,
          onPositionChange:
            cover?.features.position === true
              ? (position) => {
                  run({ capability: 'cover', action: 'set_position', position }, sensitive)
                }
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
            if (target === undefined || command === null) return
            void ctx.send(target, command, { label, confirmed: true }).then((outcome) => {
              if (outcome.ok) ctx.onOpened?.(label)
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

    case 'clock-weather':
      // Shows no place name unless the dashboard gives one.
      return { type, props: { label: stringProp(widget, 'label') } }
  }
}

/** Build the registry spec for one dashboard widget. Unknown types keep their type. */
export function bindWidget(widget: Widget, view: EndpointView | undefined, ctx: BindingContext): WidgetSpec | AnyWidgetSpec {
  if (!isWidgetType(widget.type)) return { type: widget.type, props: widget.props }
  return bindKnown(widget.type, widget, view, ctx)
}
