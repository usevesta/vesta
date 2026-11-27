/**
 * Optimistic commands: what the screen shows between a tap and the
 * device's confirmation. A command predicts (part of) the state it will
 * cause; widgets render that prediction marked as pending until a matching
 * `state` arrives from the home.
 */
import type { Capabilities, CapabilityState, Command, CoverMotion, CoverState, Endpoint } from '@/api/generated'

/** Capabilities that have a state (everything except `trigger` and `doorbell`). */
export type StatefulCapability = CapabilityState['capability']

type StateOf<K extends StatefulCapability> = Extract<CapabilityState, { capability: K }>['state']

/** Predicted (partial) state per capability while a command is pending. */
export type PredictedStates = { readonly [K in StatefulCapability]?: Readonly<Partial<StateOf<K>>> }

export interface Prediction {
  readonly predicted: PredictedStates
  /**
   * The capability whose next `state` confirms the command. Null when the
   * command causes no state at all (`trigger.fire`): it resolves on `ok`.
   */
  readonly confirmsWith: StatefulCapability | null
}

function coverMotion(command: Extract<Command, { capability: 'cover' }>, state: CoverState | undefined): CoverMotion | null {
  switch (command.action) {
    case 'open':
      return 'opening'
    case 'close':
      return 'closing'
    case 'stop':
      return 'stopped'
    case 'set_position': {
      const current = state?.position
      if (current === undefined) return null
      if (command.position === current) return 'stopped'
      return command.position > current ? 'opening' : 'closing'
    }
  }
}

/** What `command` will do to `endpoint` (as currently shown), per the capability model. */
export function predict(command: Command, endpoint: Endpoint | undefined): Prediction {
  const capabilities = endpoint?.capabilities
  // `level.set` and `fan.set_speed` also switch the endpoint on.
  const turnsOn: PredictedStates = capabilities?.on_off === undefined ? {} : { on_off: { on: true } }

  switch (command.capability) {
    case 'on_off': {
      const current = capabilities?.on_off?.state?.on
      const on =
        command.action === 'on' ? true : command.action === 'off' ? false : current === undefined ? null : !current
      return { predicted: on === null ? {} : { on_off: { on } }, confirmsWith: 'on_off' }
    }
    case 'level':
      return {
        predicted: { ...(command.level > 0 ? turnsOn : {}), level: { level: command.level } },
        confirmsWith: 'level',
      }
    case 'color_temperature':
      return { predicted: { color_temperature: { kelvin: command.kelvin } }, confirmsWith: 'color_temperature' }
    case 'color':
      return { predicted: { color: { hue: command.hue, saturation: command.saturation } }, confirmsWith: 'color' }
    case 'fan':
      return command.action === 'set_speed'
        ? { predicted: { ...turnsOn, fan: { speed: command.speed } }, confirmsWith: 'fan' }
        : { predicted: { fan: { direction: command.direction } }, confirmsWith: 'fan' }
    case 'cover': {
      const motion = coverMotion(command, capabilities?.cover?.state)
      return { predicted: motion === null ? {} : { cover: { motion } }, confirmsWith: 'cover' }
    }
    case 'lock':
      return {
        predicted: { lock: { lock_state: command.action === 'lock' ? 'locked' : 'unlocked' } },
        confirmsWith: 'lock',
      }
    case 'trigger':
      return { predicted: {}, confirmsWith: null }
    case 'ev_charger':
      return { predicted: {}, confirmsWith: 'ev_charger' }
  }
}

function isSubset(partial: Readonly<Record<string, unknown>>, full: Readonly<Record<string, unknown>>): boolean {
  return Object.entries(partial).every(([key, value]) => full[key] === value)
}

function predictedEntries(predicted: PredictedStates): [StatefulCapability, Readonly<Record<string, unknown>>][] {
  // Keys of a PredictedStates are always stateful capability names.
  return Object.entries(predicted) as [StatefulCapability, Readonly<Record<string, unknown>>][]
}

export function predictsAnything(predicted: PredictedStates): boolean {
  return Object.keys(predicted).length > 0
}

export function predictsCapability(predicted: PredictedStates, kind: StatefulCapability): boolean {
  return Object.hasOwn(predicted, kind)
}

/** True when every predicted value is already in `capabilities`. */
export function matchesPrediction(capabilities: Capabilities, predicted: PredictedStates): boolean {
  return predictedEntries(predicted).every(([kind, partial]) => {
    const state = capabilities[kind]?.state
    return state !== undefined && isSubset(partial, state)
  })
}

/**
 * Capabilities with `predicted` laid over their current state. A prediction
 * for a capability whose state is still unknown is skipped (a partial
 * prediction is not a full state).
 */
export function applyPrediction(capabilities: Capabilities, predicted: PredictedStates): Capabilities {
  const next: Record<string, unknown> = { ...capabilities }
  for (const [kind, partial] of predictedEntries(predicted)) {
    const capability = capabilities[kind]
    if (capability?.state === undefined) continue
    next[kind] = { ...capability, state: { ...capability.state, ...partial } }
  }
  // Same keys as `capabilities`; each state keeps its own shape.
  return next
}
