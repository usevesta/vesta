/**
 * Test data shaped like the simulated home (`vesta-hubd --dev`). Used by
 * unit tests only; typed with the generated API types.
 */
import type { Device, Snapshot } from '@/api/generated'
import type { SocketFactory, SocketHandlers } from '@/lib/ws/reconnecting-socket'

export function sampleDevices(): Device[] {
  return [
    {
      id: 'living_room_light',
      name: 'Living Room Light',
      room: 'living_room',
      online: true,
      endpoints: [
        {
          id: 'main',
          name: 'Living Room Light',
          type: 'light',
          capabilities: {
            on_off: { state: { on: false } },
            level: { state: { level: 80 } },
            color_temperature: { features: { min_kelvin: 2_700, max_kelvin: 6_500 }, state: { kelvin: 3_000 } },
          },
        },
      ],
    },
    {
      id: 'living_room_fan',
      name: 'Living Room Fan',
      online: true,
      endpoints: [
        {
          id: 'fan',
          name: 'Ceiling Fan',
          type: 'fan',
          capabilities: {
            on_off: { state: { on: false } },
            fan: { features: { speed_steps: 6, reversible: true }, state: { speed: 1, direction: 'forward' } },
          },
        },
        {
          id: 'light',
          name: 'Fan Light',
          type: 'light',
          capabilities: { on_off: { state: { on: false } }, level: { state: { level: 100 } } },
        },
      ],
    },
    {
      id: 'bedroom_blind',
      name: 'Bedroom Blind',
      online: true,
      endpoints: [
        {
          id: 'main',
          name: 'Bedroom Blind',
          type: 'window_covering',
          capabilities: {
            cover: { features: { kind: 'blind', position: true, stop: true }, state: { position: 100, motion: 'stopped' } },
          },
        },
      ],
    },
    {
      id: 'garage_door',
      name: 'Garage Door',
      online: true,
      endpoints: [
        {
          id: 'main',
          name: 'Garage Door',
          type: 'garage_door',
          capabilities: {
            cover: { features: { kind: 'garage', position: false, stop: false }, state: { motion: 'stopped', open: false } },
          },
        },
      ],
    },
    {
      id: 'front_door_strike',
      name: 'Front Door',
      online: true,
      endpoints: [{ id: 'main', name: 'Front Door', type: 'door', capabilities: { trigger: {} } }],
    },
    {
      id: 'front_doorbell',
      name: 'Front Doorbell',
      online: false,
      endpoints: [
        {
          id: 'main',
          name: 'Front Doorbell',
          type: 'doorbell',
          capabilities: { doorbell: {}, occupancy: { state: { occupied: false } } },
        },
      ],
    },
  ]
}

export function sampleSnapshot(): Snapshot {
  return { devices: sampleDevices() }
}

/** A socket factory whose sockets the test drives by hand. */
export class FakeSockets {
  readonly sent: unknown[] = []
  #handlers: SocketHandlers | null = null

  readonly factory: SocketFactory = (_url, handlers) => {
    this.#handlers = handlers
    return {
      send: (data) => {
        this.sent.push(JSON.parse(data))
      },
      close: () => {
        this.#handlers = null
      },
    }
  }

  open(): void {
    this.#handlers?.onOpen()
  }

  receive(message: unknown): void {
    this.#handlers?.onMessage(JSON.stringify(message))
  }

  drop(): void {
    const handlers = this.#handlers
    this.#handlers = null
    handlers?.onClose()
  }

  /** Last `command` message sent. */
  lastSent(): unknown {
    return this.sent.at(-1)
  }
}

export const NO_WAKE_SIGNALS = { subscribe: () => () => undefined }

/**
 * A car charger like the simulated one: the charger plus the home's grid,
 * house and solar meters, all endpoints of one device.
 */
export function chargerDevice(overrides: { online?: boolean } = {}): Device {
  return {
    id: 'car_charger',
    name: 'Car Charger',
    room: 'garage',
    online: overrides.online ?? true,
    endpoints: [
      {
        id: 'charger',
        name: 'Car Charger',
        type: 'ev_charger',
        capabilities: {
          ev_charger: {
            features: {
              min_current_a: 6,
              max_current_a: 32,
              modes: ['manual', 'solar', 'solar_grid'],
              pause: true,
              adjustable_current: true,
              lock: true,
              schedule: true,
            },
            state: {
              status: 'charging',
              power_w: 5_092,
              session_energy_wh: 4_890,
              session_duration_s: 3_841,
              current_a: 23,
              auto_current: false,
              phase_current_a: [22.94, 0, 0],
              min_current_a: 6,
              max_current_a: 30,
              mode: 'manual',
              locked: false,
              schedule_enabled: true,
            },
          },
        },
      },
      {
        id: 'grid',
        name: 'Grid',
        type: 'meter',
        capabilities: {
          power_meter: { features: { role: 'grid', bidirectional: true }, state: { power_w: 5_300, limit_w: 5_750 } },
        },
      },
      {
        id: 'house',
        name: 'House',
        type: 'meter',
        capabilities: { power_meter: { features: { role: 'house', bidirectional: false }, state: { power_w: 6_100 } } },
      },
      {
        id: 'solar',
        name: 'Solar panels',
        type: 'meter',
        capabilities: { power_meter: { features: { role: 'solar', bidirectional: false }, state: { power_w: 800 } } },
      },
    ],
  }
}

/** A plug with a meter, like the simulated hall lamp (or one that is only watched). */
export function plugDevice(overrides: { readOnly?: boolean; online?: boolean } = {}): Device {
  return {
    id: 'hall_lamp',
    name: 'Hall Lamp',
    room: 'hall',
    online: overrides.online ?? true,
    endpoints: [
      {
        id: 'main',
        name: 'Hall Lamp',
        type: 'outlet',
        capabilities: {
          on_off: {
            ...(overrides.readOnly === true ? { features: { read_only: true } } : {}),
            state: { on: true },
          },
          power_meter: {
            features: { role: 'load', bidirectional: false },
            state: { power_w: 12.5, voltage_v: 227.2, current_a: 0.112 },
          },
        },
      },
    ],
  }
}

/** A robot vacuum on its base (with or without controls). */
export function robotDevice(overrides: { readOnly?: boolean } = {}): Device {
  const control = overrides.readOnly !== true
  return {
    id: 'robot_vacuum',
    name: 'Robot Vacuum',
    online: true,
    endpoints: [
      {
        id: 'main',
        name: 'Robot Vacuum',
        type: 'robot_vacuum',
        capabilities: {
          robot_vacuum: {
            features: { start: control, pause: control, return_to_dock: control },
            state: { status: 'docked', cleaned_area_m2: 22, cleaning_duration_s: 1_800 },
          },
          battery: { state: { percent: 100, low: false } },
        },
      },
    ],
  }
}
