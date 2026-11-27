import { describe, expect, it } from 'vitest'

import { fetchVersionedDashboard, revisionFromEtag, saveDashboard } from '@/api/dashboard-api'
import type { Dashboard } from '@/api/generated'

const DASHBOARD: Dashboard = {
  schema_version: 1,
  theme: { preset: 'glass-dark', auto_dark_mode: true, touch_scale: 'large' },
  kiosk: { screensaver_timeout_sec: 120, wake_on: [] },
  pages: [{ id: 'home', title: 'Home', grid_columns: 4, widgets: [] }],
}

interface Call {
  url: string
  init: RequestInit | undefined
}

function fakeFetch(respond: () => Response | Promise<Response>) {
  const calls: Call[] = []
  const fetch: typeof globalThis.fetch = (input, init) => {
    calls.push({ url: typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, init })
    return Promise.resolve(respond())
  }
  return { calls, fetch }
}

function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } })
}

describe('revisionFromEtag', () => {
  it('accepts strong tags only', () => {
    expect(revisionFromEtag('"5f2c9a0e7b1d4c38"')).toBe('5f2c9a0e7b1d4c38')
    expect(revisionFromEtag(' "abc" ')).toBe('abc')
    expect(revisionFromEtag('W/"abc"')).toBeNull()
    expect(revisionFromEtag('abc')).toBeNull()
    expect(revisionFromEtag('""')).toBeNull()
    expect(revisionFromEtag(null)).toBeNull()
  })
})

describe('fetchVersionedDashboard', () => {
  it('returns the dashboard and its revision', async () => {
    const { calls, fetch } = fakeFetch(() => json(DASHBOARD, 200, { ETag: '"rev-1"' }))
    await expect(fetchVersionedDashboard({ fetch, baseUrl: 'http://127.0.0.1:8080' })).resolves.toEqual({
      dashboard: DASHBOARD,
      revision: 'rev-1',
    })
    expect(calls[0]?.url).toBe('http://127.0.0.1:8080/api/v1/dashboard')
  })

  it('works with a home that sends no revision (it just cannot save)', async () => {
    const { fetch } = fakeFetch(() => json(DASHBOARD))
    await expect(fetchVersionedDashboard({ fetch })).resolves.toEqual({ dashboard: DASHBOARD, revision: null })
  })

  it('throws on errors and on things it does not understand', async () => {
    await expect(fetchVersionedDashboard({ fetch: fakeFetch(() => json({}, 500)).fetch })).rejects.toThrow()
    await expect(fetchVersionedDashboard({ fetch: fakeFetch(() => json({ pages: 'no' })).fetch })).rejects.toThrow()
  })
})

describe('saveDashboard', () => {
  it('sends the whole dashboard with If-Match and returns the new revision', async () => {
    const saved = { ...DASHBOARD, pages: [{ id: 'home', title: 'Inicio', grid_columns: 4, widgets: [] }] }
    const { calls, fetch } = fakeFetch(() => json(saved, 200, { ETag: '"rev-2"' }))
    await expect(saveDashboard(saved, 'rev-1', { fetch })).resolves.toEqual({
      status: 'saved',
      dashboard: saved,
      revision: 'rev-2',
    })
    const init = calls[0]?.init
    expect(init?.method).toBe('PUT')
    expect(init?.headers).toMatchObject({ 'If-Match': '"rev-1"', 'Content-Type': 'application/json' })
    const body = init?.body
    if (typeof body !== 'string') throw new Error('the body is not JSON text')
    expect(JSON.parse(body)).toEqual(saved)
  })

  it('reports a conflict when someone else saved first (412)', async () => {
    const { fetch } = fakeFetch(() => json({ error: 'stale' }, 412, { ETag: '"rev-9"' }))
    await expect(saveDashboard(DASHBOARD, 'rev-1', { fetch })).resolves.toEqual({ status: 'conflict', revision: 'rev-9' })
  })

  it('sends no If-Match without a revision, and the refusal is reported', async () => {
    const { calls, fetch } = fakeFetch(() => json({ error: 'if_match_required' }, 428))
    await expect(saveDashboard(DASHBOARD, null, { fetch })).resolves.toEqual({ status: 'rejected', httpStatus: 428 })
    expect(calls[0]?.init?.headers).not.toHaveProperty('If-Match')
  })

  it('tells refusals apart from failures', async () => {
    for (const status of [400, 403, 415, 422]) {
      await expect(saveDashboard(DASHBOARD, 'r', { fetch: fakeFetch(() => json({}, status)).fetch })).resolves.toEqual({
        status: 'rejected',
        httpStatus: status,
      })
    }
    await expect(saveDashboard(DASHBOARD, 'r', { fetch: fakeFetch(() => json({}, 500)).fetch })).resolves.toEqual({
      status: 'failed',
    })
    const offline: typeof globalThis.fetch = () => Promise.reject(new TypeError('Failed to fetch'))
    await expect(saveDashboard(DASHBOARD, 'r', { fetch: offline })).resolves.toEqual({ status: 'failed' })
  })
})
