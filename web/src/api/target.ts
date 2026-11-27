import type { DeviceId, EndpointId, Target } from '@/api/generated'

export interface ParsedTarget {
  deviceId: DeviceId
  endpointId: EndpointId
}

/** `<device_id>/<endpoint_id>`, e.g. `living_room_fan/light`. */
export function makeTarget(deviceId: DeviceId, endpointId: EndpointId): Target {
  return `${deviceId}/${endpointId}`
}

/** Split a target into its device and endpoint ids; null when malformed. */
export function parseTarget(target: Target): ParsedTarget | null {
  const slash = target.indexOf('/')
  if (slash <= 0 || slash === target.length - 1 || target.includes('/', slash + 1)) return null
  return { deviceId: target.slice(0, slash), endpointId: target.slice(slash + 1) }
}
