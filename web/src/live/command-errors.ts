import type { CommandFailure } from '@/live/live-client'

/** Translation keys for failed commands. Every one takes `{{name}}` except `notConnected`. */
export type CommandErrorKey =
  | 'widget.commandFailed'
  | 'commandError.unsupported'
  | 'commandError.offline'
  | 'commandError.notAllowed'
  | 'commandError.confirmationRequired'
  | 'commandError.device'
  | 'commandError.notConnected'
  | 'ev.hours.saveFailed'

/** Plain-language message for why a command did not happen. */
export function commandErrorKey(
  failure: Pick<CommandFailure, 'reason' | 'rejectReason'> & Partial<Pick<CommandFailure, 'command'>>,
): CommandErrorKey {
  // Saving a charger's hours goes through the internet: say so.
  const savingHours =
    failure.command?.capability === 'ev_charger' && failure.command.action === 'set_charging_schedule'
  if (savingHours && ['device', 'timeout', 'no_response'].includes(failure.reason)) return 'ev.hours.saveFailed'
  switch (failure.reason) {
    case 'unsupported':
      return 'commandError.unsupported'
    case 'offline':
      return 'commandError.offline'
    case 'timeout':
    case 'no_response':
      return 'widget.commandFailed'
    case 'rejected':
      return failure.rejectReason === 'confirmation_required'
        ? 'commandError.confirmationRequired'
        : 'commandError.notAllowed'
    case 'device':
      return 'commandError.device'
    case 'not_connected':
      return 'commandError.notConnected'
  }
}
