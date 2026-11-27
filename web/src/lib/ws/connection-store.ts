import { create } from 'zustand'

import {
  INITIAL_CONNECTION_SNAPSHOT,
  type ConnectionSnapshot,
  type ReconnectingSocket,
} from '@/lib/ws/reconnecting-socket'

/**
 * Connection state for the UI (banners, status dots). Read-only from React:
 * it mirrors whichever socket was bound with `bindConnectionStore`.
 */
export const useConnectionStore = create<ConnectionSnapshot>()(() => INITIAL_CONNECTION_SNAPSHOT)

/** Mirror `socket` state into the store. Returns an unbind function. */
export function bindConnectionStore(socket: ReconnectingSocket): () => void {
  useConnectionStore.setState(socket.snapshot, true)
  return socket.onStateChange((snapshot) => {
    useConnectionStore.setState(snapshot, true)
  })
}
