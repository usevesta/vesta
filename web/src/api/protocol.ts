/**
 * Runtime guards for everything the home sends. Wire types come only from
 * `src/api/generated` (exported from Rust); this file checks that an
 * incoming value really has that shape before the app trusts it.
 *
 * Forward compatible on purpose: a newer home may send message types,
 * capabilities, events or endpoint types this version does not know. Those
 * are dropped (or skipped inside a list) instead of breaking the screen.
 */
import type {
  Capabilities,
  CapabilityKind,
  CommandError,
  CommandResult,
  Dashboard,
  Device,
  DoorbellOverlay,
  Endpoint,
  EndpointType,
  EventKind,
  EventRef,
  JsonValue,
  Kiosk,
  Page,
  RejectReason,
  ServerMessage,
  Snapshot,
  StateChange,
  Theme,
  TouchScale,
  Widget,
} from '@/api/generated'

type UnknownRecord = Record<string, unknown>

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isString(value: unknown): value is string {
  return typeof value === 'string'
}

function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0
}

function optionalString(value: unknown): value is string | undefined {
  return value === undefined || typeof value === 'string'
}

function isOneOf<T extends string>(set: Readonly<Record<T, unknown>>, value: unknown): value is T {
  return typeof value === 'string' && Object.hasOwn(set, value)
}

/** Every capability, and whether its wire form must carry `features`. */
const CAPABILITY_FEATURES: Readonly<Record<CapabilityKind, boolean>> = {
  on_off: false,
  level: false,
  color_temperature: true,
  color: false,
  cover: true,
  fan: true,
  lock: true,
  trigger: false,
  camera: true,
  doorbell: false,
  occupancy: false,
  contact: false,
  temperature: false,
  humidity: false,
  power_meter: true,
  ev_charger: true,
  battery: false,
}

/** Capabilities that have a state (and so appear in `state` messages). */
const STATEFUL_CAPABILITIES: Readonly<Record<StateChange['capability'], true>> = {
  on_off: true,
  level: true,
  color_temperature: true,
  color: true,
  cover: true,
  fan: true,
  lock: true,
  camera: true,
  occupancy: true,
  contact: true,
  temperature: true,
  humidity: true,
  power_meter: true,
  ev_charger: true,
  battery: true,
}

const EVENT_KINDS: Readonly<Record<EventKind, true>> = {
  'doorbell.pressed': true,
  'occupancy.detected': true,
}

const ENDPOINT_TYPES: Readonly<Record<EndpointType, true>> = {
  light: true,
  outlet: true,
  switch: true,
  fan: true,
  window_covering: true,
  garage_door: true,
  gate: true,
  door: true,
  lock: true,
  camera: true,
  doorbell: true,
  sensor: true,
  meter: true,
  ev_charger: true,
}

const ERROR_CODES: Readonly<Record<CommandError['code'], true>> = {
  unsupported: true,
  offline: true,
  timeout: true,
  rejected: true,
  device: true,
}

const REJECT_REASONS: Readonly<Record<RejectReason, true>> = {
  not_allowed: true,
  confirmation_required: true,
}

const TOUCH_SCALES: Readonly<Record<TouchScale, true>> = {
  normal: true,
  large: true,
  extra_large: true,
}

export function isEventKind(value: unknown): value is EventKind {
  return isOneOf(EVENT_KINDS, value)
}

export function decodeCapabilities(value: unknown): Capabilities | null {
  if (!isRecord(value)) return null
  const capabilities: UnknownRecord = {}
  for (const [kind, capability] of Object.entries(value)) {
    // A capability this version does not know: ignore it, keep the rest.
    if (!isOneOf(CAPABILITY_FEATURES, kind)) continue
    if (!isRecord(capability)) return null
    const { features, state } = capability
    if (CAPABILITY_FEATURES[kind] ? !isRecord(features) : features !== undefined && !isRecord(features)) {
      return null
    }
    if (state !== undefined && !isRecord(state)) return null
    capabilities[kind] = capability
  }
  // Every key is a known capability whose features/state are objects.
  return capabilities
}

export function decodeEndpoint(value: unknown): Endpoint | null {
  if (!isRecord(value)) return null
  const { id, name, type, room } = value
  if (!isString(id) || !isString(name) || !isOneOf(ENDPOINT_TYPES, type) || !optionalString(room)) return null
  const capabilities = decodeCapabilities(value.capabilities)
  if (capabilities === null) return null
  return { id, name, type, capabilities, ...(room === undefined ? {} : { room }) }
}

export function decodeDevice(value: unknown): Device | null {
  if (!isRecord(value)) return null
  const { id, name, room, online, last_seen: lastSeen, endpoints } = value
  if (
    !isString(id) ||
    !isString(name) ||
    typeof online !== 'boolean' ||
    !optionalString(room) ||
    !optionalString(lastSeen) ||
    !Array.isArray(endpoints)
  ) {
    return null
  }
  return {
    id,
    name,
    online,
    // Endpoints this version cannot show (e.g. a newer type) are skipped.
    endpoints: endpoints.map(decodeEndpoint).filter((endpoint) => endpoint !== null),
    ...(room === undefined ? {} : { room }),
    ...(lastSeen === undefined ? {} : { last_seen: lastSeen }),
  }
}

export function decodeSnapshot(value: unknown): Snapshot | null {
  if (!isRecord(value) || !Array.isArray(value.devices)) return null
  return { devices: value.devices.map(decodeDevice).filter((device) => device !== null) }
}

function decodeStateChange(value: UnknownRecord): StateChange | null {
  const { target, capability, state } = value
  if (!isString(target) || !isOneOf(STATEFUL_CAPABILITIES, capability) || !isRecord(state)) return null
  // `capability` and `state` travel together as generated by the home.
  return { target, capability, state } as StateChange
}

function decodeCommandError(value: unknown): CommandError {
  if (!isRecord(value) || !isOneOf(ERROR_CODES, value.code)) {
    // Unknown failure from a newer home: still a failure the user must see.
    return { code: 'device' }
  }
  switch (value.code) {
    case 'rejected':
      return { code: 'rejected', reason: isOneOf(REJECT_REASONS, value.reason) ? value.reason : 'not_allowed' }
    case 'device':
      // `message` is for diagnostics only and never shown; drop it.
      return { code: 'device' }
    default:
      return { code: value.code }
  }
}

function decodeCommandResult(value: UnknownRecord): CommandResult | null {
  const { request_id: requestId, ok } = value
  if (!isString(requestId) || typeof ok !== 'boolean') return null
  return ok ? { request_id: requestId, ok } : { request_id: requestId, ok, error: decodeCommandError(value.error) }
}

/**
 * Check one parsed WebSocket message from the home. Returns null for
 * anything that is not a well-formed `ServerMessage` this version knows.
 */
export function decodeServerMessage(value: unknown): ServerMessage | null {
  if (!isRecord(value)) return null
  switch (value.type) {
    case 'snapshot': {
      const snapshot = decodeSnapshot(value)
      return snapshot === null ? null : { type: 'snapshot', ...snapshot }
    }
    case 'state': {
      const change = decodeStateChange(value)
      return change === null ? null : { type: 'state', ...change }
    }
    case 'event': {
      const { target, event, at } = value
      if (!isString(target) || !isEventKind(event) || !isString(at)) return null
      return { type: 'event', target, event, at }
    }
    case 'availability': {
      const { device_id: deviceId, online } = value
      if (!isString(deviceId) || typeof online !== 'boolean') return null
      return { type: 'availability', device_id: deviceId, online }
    }
    case 'command_result': {
      const result = decodeCommandResult(value)
      return result === null ? null : { type: 'command_result', ...result }
    }
    case 'dashboard_changed': {
      const { revision } = value
      return isString(revision) && revision !== '' ? { type: 'dashboard_changed', revision } : null
    }
    default:
      return null
  }
}

function decodeTheme(value: unknown): Theme | null {
  if (!isRecord(value)) return null
  const { preset, auto_dark_mode: autoDarkMode, touch_scale: touchScale } = value
  if (!isString(preset) || typeof autoDarkMode !== 'boolean') return null
  return {
    preset,
    auto_dark_mode: autoDarkMode,
    touch_scale: isOneOf(TOUCH_SCALES, touchScale) ? touchScale : 'normal',
  }
}

function decodeEventRef(value: unknown): EventRef | null {
  if (!isRecord(value)) return null
  const { target, event } = value
  return isString(target) && isEventKind(event) ? { target, event } : null
}

function decodeKiosk(value: unknown): Kiosk | null {
  if (!isRecord(value)) return null
  const { screensaver_timeout_sec: timeout, wake_on: wakeOn = [] } = value
  if (!isCount(timeout) || !Array.isArray(wakeOn)) return null
  return {
    screensaver_timeout_sec: timeout,
    // Events this version does not know are skipped.
    wake_on: wakeOn.map(decodeEventRef).filter((ref) => ref !== null),
  }
}

function decodeDoorbellOverlay(value: unknown): DoorbellOverlay | null {
  if (!isRecord(value)) return null
  const {
    doorbell_target: doorbellTarget,
    camera_target: cameraTarget,
    open_target: openTarget,
    auto_dismiss_sec: autoDismiss,
  } = value
  if (!isString(doorbellTarget) || !isCount(autoDismiss) || !optionalString(cameraTarget) || !optionalString(openTarget)) {
    return null
  }
  return {
    doorbell_target: doorbellTarget,
    auto_dismiss_sec: autoDismiss,
    ...(cameraTarget === undefined ? {} : { camera_target: cameraTarget }),
    ...(openTarget === undefined ? {} : { open_target: openTarget }),
  }
}

function decodeWidget(value: unknown): Widget | null {
  if (!isRecord(value)) return null
  const { type, target, col_span: colSpan, row_span: rowSpan, props } = value
  if (!isString(type) || !optionalString(target) || !isCount(colSpan) || !isCount(rowSpan)) return null
  if (props !== undefined && !isRecord(props)) return null
  return {
    type,
    col_span: colSpan,
    row_span: rowSpan,
    ...(target === undefined ? {} : { target }),
    // Parsed JSON: every value is a JsonValue.
    ...(props === undefined ? {} : { props: props as Record<string, JsonValue> }),
  }
}

function decodePage(value: unknown): Page | null {
  if (!isRecord(value)) return null
  const { id, title, icon, grid_columns: columns, widgets } = value
  if (!isString(id) || !isString(title) || !optionalString(icon) || !isCount(columns) || !Array.isArray(widgets)) {
    return null
  }
  const decoded = widgets.map(decodeWidget)
  if (decoded.some((widget) => widget === null)) return null
  return {
    id,
    title,
    grid_columns: Math.max(1, columns),
    widgets: decoded.filter((widget) => widget !== null),
    ...(icon === undefined ? {} : { icon }),
  }
}

/** Check the body of `GET /api/v1/dashboard`. */
export function decodeDashboard(value: unknown): Dashboard | null {
  if (!isRecord(value) || !isCount(value.schema_version) || !Array.isArray(value.pages)) return null
  const theme = decodeTheme(value.theme)
  const kiosk = decodeKiosk(value.kiosk)
  const pages = value.pages.map(decodePage)
  if (theme === null || kiosk === null || pages.some((page) => page === null)) return null
  const overlay = value.doorbell_overlay === undefined ? undefined : decodeDoorbellOverlay(value.doorbell_overlay)
  if (overlay === null) return null
  return {
    schema_version: value.schema_version,
    theme,
    kiosk,
    pages: pages.filter((page) => page !== null),
    ...(overlay === undefined ? {} : { doorbell_overlay: overlay }),
  }
}
