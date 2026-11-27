/** Path of the home's WebSocket, relative to the local endpoint origin. */
export const WS_PATH = '/api/v1/ws'

/**
 * Build the WebSocket URL on the same origin the app was served from. The
 * frontend always talks to a local endpoint (the home itself, a local proxy,
 * or the Vite dev proxy), so the origin is the right place.
 */
export function sameOriginSocketUrl(
  location: Pick<Location, 'protocol' | 'host'>,
  path: string = WS_PATH,
): string {
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${protocol}//${location.host}${path}`
}
