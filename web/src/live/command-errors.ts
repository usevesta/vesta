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

/** Plain-language message for why a command did not happen. */
export function commandErrorKey(failure: Pick<CommandFailure, 'reason' | 'rejectReason'>): CommandErrorKey {
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
