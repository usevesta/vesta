/**
 * Loading and saving the dashboard together with its revision (the
 * `ETag`), so two people editing at once never silently overwrite each
 * other: a save names the revision it started from (`If-Match`) and the
 * home refuses it (`412`) when someone else saved first.
 *
 * Same base path and request options as `http.ts`. If requests ever need
 * extra headers (e.g. a session token), they belong here too.
 */
import type { Dashboard } from '@/api/generated'
import { API_BASE_PATH, UnexpectedResponseError, type RequestOptions } from '@/api/http'
import { decodeDashboard } from '@/api/protocol'

const PATH = '/dashboard'

export interface VersionedDashboard {
  readonly dashboard: Dashboard
  /** The `ETag` without quotes; null when the home sent none (it then cannot be saved). */
  readonly revision: string | null
}

export type SaveDashboardResult =
  | { readonly status: 'saved'; readonly dashboard: Dashboard; readonly revision: string | null }
  /** Someone else saved first (`412`): reload and tell the person. */
  | { readonly status: 'conflict'; readonly revision: string | null }
  /** The home refused the request itself (`4xx`): not something to retry as is. */
  | { readonly status: 'rejected'; readonly httpStatus: number }
  /** The home could not be reached, or could not store it (`5xx`). */
  | { readonly status: 'failed' }

/** A strong `ETag` (`"abc"`) → `abc`. Weak or malformed tags cannot be used for saving. */
export function revisionFromEtag(etag: string | null): string | null {
  if (etag === null) return null
  const match = /^"([^"]+)"$/.exec(etag.trim())
  return match?.[1] ?? null
}

function url(options: RequestOptions): string {
  return `${options.baseUrl ?? ''}${API_BASE_PATH}${PATH}`
}

/** `GET /api/v1/dashboard`, with the revision to save against. */
export async function fetchVersionedDashboard(options: RequestOptions = {}): Promise<VersionedDashboard> {
  const fetchImpl = options.fetch ?? fetch
  const response = await fetchImpl(url(options), {
    headers: { Accept: 'application/json' },
    signal: options.signal,
  })
  if (!response.ok) throw new UnexpectedResponseError(PATH, response.status)
  const dashboard = decodeDashboard(await response.json())
  if (dashboard === null) throw new UnexpectedResponseError(PATH, response.status)
  return { dashboard, revision: revisionFromEtag(response.headers.get('ETag')) }
}

/**
 * `PUT /api/v1/dashboard`: replace the dashboard if `revision` is still the
 * current one. Never throws: every outcome is a `SaveDashboardResult`.
 */
export async function saveDashboard(
  dashboard: Dashboard,
  revision: string | null,
  options: RequestOptions = {},
): Promise<SaveDashboardResult> {
  const fetchImpl = options.fetch ?? fetch
  const headers: Record<string, string> = { Accept: 'application/json', 'Content-Type': 'application/json' }
  if (revision !== null) headers['If-Match'] = `"${revision}"`
  let response: Response
  try {
    response = await fetchImpl(url(options), {
      method: 'PUT',
      headers,
      body: JSON.stringify(dashboard),
      signal: options.signal,
    })
  } catch {
    return { status: 'failed' }
  }

  const newRevision = revisionFromEtag(response.headers.get('ETag'))
  if (response.status === 412) return { status: 'conflict', revision: newRevision }
  if (response.status >= 500) return { status: 'failed' }
  if (!response.ok) return { status: 'rejected', httpStatus: response.status }

  let saved: Dashboard | null
  try {
    saved = decodeDashboard(await response.json())
  } catch {
    saved = null
  }
  // Stored either way; fall back to what was sent if the answer is odd.
  return { status: 'saved', dashboard: saved ?? dashboard, revision: newRevision }
}
