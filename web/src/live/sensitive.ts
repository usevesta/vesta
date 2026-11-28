/**
 * Sensitive commands (docs/architecture/capabilities.md): opening or
 * unlocking something that guards access to the home. The home enforces it
 * (rejecting them without `confirmed: true`); the screen asks the person
 * first and only then sends the command marked as confirmed.
 */
import type { Command, Endpoint, EndpointType } from '@/api/generated'

const ACCESS_TYPES: ReadonlySet<EndpointType> = new Set<EndpointType>(['garage_door', 'gate', 'door', 'lock'])

/** Endpoint types whose opening is sensitive. */
export function guardsAccess(type: EndpointType): boolean {
  return ACCESS_TYPES.has(type)
}

export function isSensitiveCommand(type: EndpointType, command: Command): boolean {
  switch (command.capability) {
    case 'trigger':
      return true
    case 'lock':
      return command.action === 'unlock' || command.action === 'unlatch'
    case 'cover':
      return guardsAccess(type) && (command.action === 'open' || command.action === 'set_position')
    case 'ev_charger':
      // Anyone could charge afterwards.
      return command.action === 'unlock'
    default:
      return false
  }
}

/** The command that opens a door, gate or lock; null if the endpoint cannot open. */
export function openCommand(endpoint: Endpoint): Command | null {
  const { capabilities } = endpoint
  if (capabilities.trigger !== undefined) return { capability: 'trigger', action: 'fire' }
  if (capabilities.lock !== undefined) {
    return { capability: 'lock', action: capabilities.lock.features.unlatch ? 'unlatch' : 'unlock' }
  }
  if (capabilities.cover !== undefined) return { capability: 'cover', action: 'open' }
  return null
}
