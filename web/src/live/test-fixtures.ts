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
