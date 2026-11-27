import { describe, expect, it } from 'vitest'

import type { Endpoint } from '@/api/generated'
import { applyPrediction, matchesPrediction, predict } from '@/live/optimistic'
import { sampleDevices } from '@/live/test-fixtures'

function endpoint(deviceId: string, endpointId: string): Endpoint {
  const found = sampleDevices()
    .find((device) => device.id === deviceId)
    ?.endpoints.find((candidate) => candidate.id === endpointId)
  if (found === undefined) throw new Error(`no ${deviceId}/${endpointId}`)
  return found
}

describe('predict', () => {
  const light = endpoint('living_room_light', 'main')
  const fan = endpoint('living_room_fan', 'fan')
  const blind = endpoint('bedroom_blind', 'main')

  it('predicts on/off and toggles from the current state', () => {
    expect(predict({ capability: 'on_off', action: 'on' }, light)).toEqual({
      predicted: { on_off: { on: true } },
      confirmsWith: 'on_off',
    })
    expect(predict({ capability: 'on_off', action: 'toggle' }, light).predicted).toEqual({ on_off: { on: true } })
    // Unknown current state: nothing to predict, still waits for the state.
    expect(predict({ capability: 'on_off', action: 'toggle' }, undefined)).toEqual({
      predicted: {},
      confirmsWith: 'on_off',
    })
  })

  it('switches the light on when dimming above 0', () => {
    expect(predict({ capability: 'level', action: 'set', level: 30 }, light).predicted).toEqual({
      on_off: { on: true },
      level: { level: 30 },
    })
    expect(predict({ capability: 'level', action: 'set', level: 0 }, light).predicted).toEqual({ level: { level: 0 } })
  })

  it('starts the fan when picking a speed, not when reversing', () => {
    expect(predict({ capability: 'fan', action: 'set_speed', speed: 4 }, fan).predicted).toEqual({
      on_off: { on: true },
      fan: { speed: 4 },
    })
    expect(predict({ capability: 'fan', action: 'set_direction', direction: 'reverse' }, fan).predicted).toEqual({
      fan: { direction: 'reverse' },
    })
  })

  it('predicts cover motion only', () => {
    expect(predict({ capability: 'cover', action: 'close' }, blind).predicted).toEqual({ cover: { motion: 'closing' } })
    expect(predict({ capability: 'cover', action: 'set_position', position: 40 }, blind).predicted).toEqual({
      cover: { motion: 'closing' },
    })
    expect(predict({ capability: 'cover', action: 'set_position', position: 100 }, blind).predicted).toEqual({
      cover: { motion: 'stopped' },
    })
  })

  it('has no state to wait for on a door strike', () => {
    expect(predict({ capability: 'trigger', action: 'fire' }, undefined)).toEqual({ predicted: {}, confirmsWith: null })
  })
})

describe('applyPrediction / matchesPrediction', () => {
  const blind = endpoint('bedroom_blind', 'main')

  it('lays partial predictions over the current state', () => {
    const shown = applyPrediction(blind.capabilities, { cover: { motion: 'closing' } })
    expect(shown.cover?.state).toEqual({ position: 100, motion: 'closing' })
    expect(blind.capabilities.cover?.state?.motion).toBe('stopped')
  })

  it('skips capabilities whose state is unknown', () => {
    const shown = applyPrediction({ on_off: {} }, { on_off: { on: true } })
    expect(shown).toEqual({ on_off: {} })
  })

  it('matches only the predicted fields', () => {
    expect(matchesPrediction(blind.capabilities, { cover: { position: 100 } })).toBe(true)
    expect(matchesPrediction(blind.capabilities, { cover: { motion: 'opening' } })).toBe(false)
    expect(matchesPrediction(blind.capabilities, { on_off: { on: true } })).toBe(false)
  })
})
