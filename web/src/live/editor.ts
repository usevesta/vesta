/**
 * The screen editor against the real home: saving goes to
 * `PUT /api/v1/dashboard`, the things that can be added come from the
 * device store.
 */
import { useMemo } from 'react'

import { saveDashboard } from '@/api/dashboard-api'
import type { CapabilityKind } from '@/api/generated'
import { parseTarget } from '@/api/target'
import type { SaveDashboard } from '@/editor/editor-store'
import { roomNameFromId, type HomeItem } from '@/editor/model'
import { reloadDashboard, setDashboard } from '@/live/dashboard-store'
import { useDeviceStore, type DeviceData } from '@/live/device-store'

/** Save to the home. On a conflict, load the latest version so the screen shows it. */
export const saveLiveDashboard: SaveDashboard = async (dashboard, baseRevision) => {
  const result = await saveDashboard(dashboard, baseRevision)
  switch (result.status) {
    case 'saved':
      setDashboard(result.dashboard, result.revision)
      return { ok: true }
    case 'conflict':
      await reloadDashboard()
      return { ok: false, reason: 'conflict' }
    case 'rejected':
    case 'failed':
      return { ok: false, reason: 'failed' }
  }
}

/** Every endpoint as something a tile can show, in the home's order. */
export function homeItems(data: Pick<DeviceData, 'devices' | 'endpoints'>): HomeItem[] {
  const parts = new Map<string, number>()
  const parsed = Object.keys(data.endpoints).map((target) => {
    const deviceId = parseTarget(target)?.deviceId
    if (deviceId !== undefined) parts.set(deviceId, (parts.get(deviceId) ?? 0) + 1)
    return { target, deviceId }
  })

  const items: HomeItem[] = []
  for (const { target, deviceId } of parsed) {
    const endpoint = data.endpoints[target]
    const device = deviceId === undefined ? undefined : data.devices[deviceId]
    if (endpoint === undefined || device === undefined || deviceId === undefined) continue
    const roomId = endpoint.room ?? device.room
    const partOf = (parts.get(deviceId) ?? 0) > 1 && device.name !== endpoint.name ? device.name : undefined
    items.push({
      target,
      name: endpoint.name,
      type: endpoint.type,
      // The protocol guard only keeps capabilities this version knows.
      capabilities: Object.keys(endpoint.capabilities) as CapabilityKind[],
      ...(partOf === undefined ? {} : { partOf }),
      ...(roomId === undefined ? {} : { room: { id: roomId, name: roomNameFromId(roomId) } }),
    })
  }
  return items
}

/** The home's things for the editor, and whether the list reflects the home yet. */
export function useHomeItems(): { items: HomeItem[]; ready: boolean } {
  const devices = useDeviceStore((s) => s.devices)
  const endpoints = useDeviceStore((s) => s.endpoints)
  const synced = useDeviceStore((s) => s.synced)
  const items = useMemo(() => homeItems({ devices, endpoints }), [devices, endpoints])
  // Once a snapshot arrived, the list stays usable while reconnecting.
  return { items, ready: synced || Object.keys(devices).length > 0 }
}
