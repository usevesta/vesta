import type { Dashboard, Snapshot } from '@/api/generated'
import { decodeDashboard, decodeSnapshot } from '@/api/protocol'

/** HTTP base path of the home's local endpoint. */
export const API_BASE_PATH = '/api/v1'

export interface RequestOptions {
  /** Origin to talk to, e.g. `http://127.0.0.1:8080`. Default: same origin. */
  baseUrl?: string
  signal?: AbortSignal
  fetch?: typeof fetch
}

/** The home answered, but not with something this version understands. */
export class UnexpectedResponseError extends Error {
  readonly path: string
  readonly status: number

  constructor(path: string, status: number) {
    super(`Unexpected response from ${path} (HTTP ${String(status)})`)
    this.name = 'UnexpectedResponseError'
    this.path = path
    this.status = status
  }
}

async function getJson<T>(path: string, decode: (value: unknown) => T | null, options: RequestOptions): Promise<T> {
  const fetchImpl = options.fetch ?? fetch
  const url = `${options.baseUrl ?? ''}${API_BASE_PATH}${path}`
  const response = await fetchImpl(url, {
    headers: { Accept: 'application/json' },
    signal: options.signal,
  })
  if (!response.ok) throw new UnexpectedResponseError(path, response.status)
  const decoded = decode(await response.json())
  if (decoded === null) throw new UnexpectedResponseError(path, response.status)
  return decoded
}

/** `GET /api/v1/dashboard`: pages, widgets, kiosk and doorbell settings. */
export function fetchDashboard(options: RequestOptions = {}): Promise<Dashboard> {
  return getJson('/dashboard', decodeDashboard, options)
}

/** `GET /api/v1/devices`: every device with its current state. */
export function fetchDevices(options: RequestOptions = {}): Promise<Snapshot> {
  return getJson('/devices', decodeSnapshot, options)
}
