import { useState, useSyncExternalStore } from 'react'

import { CommandGate, type CommandHandler } from '@/lib/command-gate'

export interface CommandControl<A extends readonly unknown[]> {
  /** A command from this control is in flight: show it and refuse taps. */
  readonly busy: boolean
  /** The arguments of the command in flight (e.g. the mode being set), or null. */
  readonly busyWith: A | null
  /** Sends, unless a command from this control is still in flight. */
  readonly run: (...args: A) => void
}

/**
 * Wraps a control's callback so the control is busy while its command is
 * in flight (see `CommandGate`). One per control: a tile's other controls
 * stay usable (e.g. Stop while a door is opening).
 */
export function useCommand<A extends readonly unknown[] = []>(
  handler: CommandHandler<A> | undefined,
): CommandControl<A> {
  const [gate] = useState(() => new CommandGate<A>())
  const busyWith = useSyncExternalStore(gate.subscribe, gate.getSnapshot, gate.getSnapshot)
  return {
    busy: busyWith !== null,
    busyWith,
    run: (...args: A) => {
      if (handler === undefined) return
      gate.run(args, () => handler(...args))
    },
  }
}
