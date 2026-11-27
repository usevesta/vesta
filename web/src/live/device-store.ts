/**
 * Live device state: devices by id, endpoints by target, plus the
 * optimistic commands still waiting for the device.
 *
 * The reducers are pure functions over `DeviceData` so they can be tested
 * without a socket. `createLiveClient` feeds them from the WebSocket.
 */
import { useMemo } from 'react'
import { create, type StoreApi } from 'zustand'

import type {
  AvailabilityChange,
  Device,
  DeviceId,
  Endpoint,
  Snapshot,
  StateChange,
  Target,
} from '@/api/generated'
import { makeTarget, parseTarget } from '@/api/target'
import {
  applyPrediction,
  matchesPrediction,
  predictsAnything,
  predictsCapability,
  type PredictedStates,
  type StatefulCapability,
} from '@/live/optimistic'

export type DeviceSummary = Omit<Device, 'endpoints'>

export interface PendingCommand {
  readonly requestId: string
  readonly target: Target
  readonly predicted: PredictedStates
  readonly confirmsWith: StatefulCapability | null
  /** The home answered `ok`: the device accepted the command. */
  readonly accepted: boolean
  /** A state for a predicted capability (or a snapshot) arrived after sending. */
  readonly observed: boolean
  /** A state for `confirmsWith` arrived after sending. */
  readonly confirmed: boolean
}

export interface DeviceData {
  /** A snapshot arrived on the current connection, so the data is live. */
  readonly synced: boolean
  readonly devices: Readonly<Record<DeviceId, DeviceSummary>>
  /** Confirmed state, as last reported by the home. */
  readonly endpoints: Readonly<Record<Target, Endpoint>>
  /** Optimistic commands per target, oldest first. */
  readonly pending: Readonly<Record<Target, readonly PendingCommand[]>>
}

export const EMPTY_DEVICE_DATA: DeviceData = Object.freeze({
  synced: false,
  devices: {},
  endpoints: {},
  pending: {},
})

export type DeviceStore = StoreApi<DeviceData>

/** A command is settled when the device has visibly done it. */
function isSettled(entry: PendingCommand, endpoint: Endpoint | undefined): boolean {
  if (entry.accepted && (entry.confirmsWith === null || entry.confirmed)) return true
  return (
    entry.observed &&
    endpoint !== undefined &&
    predictsAnything(entry.predicted) &&
    matchesPrediction(endpoint.capabilities, entry.predicted)
  )
}

/** `pending` with `target`'s list replaced (and dropped when empty). */
function withEntries(
  pending: DeviceData['pending'],
  target: Target,
  entries: readonly PendingCommand[],
): DeviceData['pending'] {
  if (entries.length > 0) return { ...pending, [target]: entries }
  return Object.fromEntries(Object.entries(pending).filter(([key]) => key !== target))
}

function settleTarget(
  pending: DeviceData['pending'],
  target: Target,
  endpoint: Endpoint | undefined,
  update: (entry: PendingCommand) => PendingCommand,
): DeviceData['pending'] {
  const entries = pending[target]
  if (entries === undefined) return pending
  return withEntries(
    pending,
    target,
    entries.map(update).filter((entry) => !isSettled(entry, endpoint)),
  )
}

/** Replace everything with a fresh snapshot (sent on every (re)connect). */
export function applySnapshot(data: DeviceData, snapshot: Snapshot): DeviceData {
  const devices: Record<DeviceId, DeviceSummary> = {}
  const endpoints: Record<Target, Endpoint> = {}
  for (const { endpoints: deviceEndpoints, ...device } of snapshot.devices) {
    devices[device.id] = device
    for (const endpoint of deviceEndpoints) endpoints[makeTarget(device.id, endpoint.id)] = endpoint
  }
  let pending = data.pending
  for (const target of Object.keys(pending)) {
    pending = settleTarget(pending, target, endpoints[target], (entry) => ({ ...entry, observed: true }))
  }
  return { synced: true, devices, endpoints, pending }
}

/** New full state of one capability. */
export function applyStateChange(data: DeviceData, change: StateChange): DeviceData {
  const { target, capability } = change
  const endpoint = data.endpoints[target]
  const current = endpoint?.capabilities[capability]
  // Unknown target, or a capability the endpoint never reported: ignore.
  if (endpoint === undefined || current === undefined) return data
  const updated: Endpoint = {
    ...endpoint,
    // `change` pairs each capability with its own state type.
    capabilities: { ...endpoint.capabilities, [capability]: { ...current, state: change.state } },
  }
  const pending = settleTarget(data.pending, target, updated, (entry) => ({
    ...entry,
    observed: entry.observed || entry.confirmsWith === capability || predictsCapability(entry.predicted, capability),
    confirmed: entry.confirmed || entry.confirmsWith === capability,
  }))
  return { ...data, endpoints: { ...data.endpoints, [target]: updated }, pending }
}

export function applyAvailability(data: DeviceData, change: AvailabilityChange): DeviceData {
  const device = data.devices[change.device_id]
  if (device === undefined || device.online === change.online) return data
  return { ...data, devices: { ...data.devices, [change.device_id]: { ...device, online: change.online } } }
}

/** The connection dropped or reopened: wait for the next snapshot. */
export function markUnsynced(data: DeviceData): DeviceData {
  return data.synced ? { ...data, synced: false } : data
}

export function addPending(data: DeviceData, entry: PendingCommand): DeviceData {
  const entries = data.pending[entry.target] ?? []
  return { ...data, pending: { ...data.pending, [entry.target]: [...entries, entry] } }
}

export function findPending(data: DeviceData, requestId: string): PendingCommand | undefined {
  for (const entries of Object.values(data.pending)) {
    const entry = entries.find((candidate) => candidate.requestId === requestId)
    if (entry !== undefined) return entry
  }
  return undefined
}

/** The home answered `ok`. */
export function acceptPending(data: DeviceData, requestId: string): DeviceData {
  const entry = findPending(data, requestId)
  if (entry === undefined) return data
  const pending = settleTarget(data.pending, entry.target, data.endpoints[entry.target], (candidate) =>
    candidate.requestId === requestId ? { ...candidate, accepted: true } : candidate,
  )
  return { ...data, pending }
}

/** Forget a command's prediction: the screen goes back to the confirmed state. */
export function removePending(data: DeviceData, requestId: string): DeviceData {
  const entry = findPending(data, requestId)
  if (entry === undefined) return data
  const remaining = (data.pending[entry.target] ?? []).filter((candidate) => candidate.requestId !== requestId)
  return { ...data, pending: withEntries(data.pending, entry.target, remaining) }
}

/** Everything a widget needs to know about one endpoint. */
export interface EndpointView {
  readonly target: Target
  readonly device: DeviceSummary
  /** With pending predictions applied, so taps show up immediately. */
  readonly endpoint: Endpoint
  /** Live data and the device is online. False renders the widget unavailable. */
  readonly available: boolean
  /** A command is waiting for the device. */
  readonly pending: boolean
}

const NO_PENDING: readonly PendingCommand[] = Object.freeze([])

export function endpointView(
  target: Target,
  endpoint: Endpoint | undefined,
  device: DeviceSummary | undefined,
  pending: readonly PendingCommand[],
  synced: boolean,
): EndpointView | undefined {
  if (endpoint === undefined || device === undefined) return undefined
  const capabilities = pending.reduce((caps, entry) => applyPrediction(caps, entry.predicted), endpoint.capabilities)
  return {
    target,
    device,
    endpoint: capabilities === endpoint.capabilities ? endpoint : { ...endpoint, capabilities },
    available: synced && device.online,
    pending: pending.length > 0,
  }
}

export function selectEndpointView(data: DeviceData, target: Target): EndpointView | undefined {
  const deviceId = parseTarget(target)?.deviceId
  return endpointView(
    target,
    data.endpoints[target],
    deviceId === undefined ? undefined : data.devices[deviceId],
    data.pending[target] ?? NO_PENDING,
    data.synced,
  )
}

export function createDeviceStore() {
  return create<DeviceData>()(() => EMPTY_DEVICE_DATA)
}

/** The app's device state (one per screen). */
export const useDeviceStore = createDeviceStore()

/** Live view of one endpoint, or undefined while it is unknown. */
export function useEndpointView(target: Target | undefined): EndpointView | undefined {
  const deviceId = target === undefined ? undefined : parseTarget(target)?.deviceId
  const endpoint = useDeviceStore((s) => (target === undefined ? undefined : s.endpoints[target]))
  const device = useDeviceStore((s) => (deviceId === undefined ? undefined : s.devices[deviceId]))
  const pending = useDeviceStore((s) => (target === undefined ? undefined : s.pending[target])) ?? NO_PENDING
  const synced = useDeviceStore((s) => s.synced)
  return useMemo(
    () => (target === undefined ? undefined : endpointView(target, endpoint, device, pending, synced)),
    [target, endpoint, device, pending, synced],
  )
}
