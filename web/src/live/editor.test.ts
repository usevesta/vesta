import { describe, expect, it } from 'vitest'

import { groupByRoom } from '@/editor/model'
import { applySnapshot, EMPTY_DEVICE_DATA } from '@/live/device-store'
import { homeItems } from '@/live/editor'
import { sampleSnapshot } from '@/live/test-fixtures'

describe('homeItems', () => {
  const items = homeItems(applySnapshot(EMPTY_DEVICE_DATA, sampleSnapshot()))

  it('lists every endpoint with plain names, never ids', () => {
    expect(items.map((item) => item.name)).toEqual([
      'Living Room Light',
      'Ceiling Fan',
      'Fan Light',
      'Bedroom Blind',
      'Garage Door',
      'Front Door',
      'Front Doorbell',
    ])
    // A part of a bigger device says which one.
    expect(items.find((item) => item.target === 'living_room_fan/light')?.partOf).toBe('Living Room Fan')
    expect(items.find((item) => item.target === 'living_room_light/main')?.partOf).toBeUndefined()
  })

  it('carries the room, type and capabilities the editor needs', () => {
    expect(items[0]).toEqual({
      target: 'living_room_light/main',
      name: 'Living Room Light',
      type: 'light',
      capabilities: ['on_off', 'level', 'color_temperature'],
      room: { id: 'living_room', name: 'Living room' },
    })
    const groups = groupByRoom(items)
    expect(groups.map((group) => group.room?.name ?? null)).toEqual(['Living room', null])
    // The doorbell has no camera yet but still gets a (placeholder) camera tile.
    expect(groups.flatMap((group) => group.items).map((item) => item.target)).toContain('front_doorbell/main')
  })
})
