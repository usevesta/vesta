import { beforeEach, describe, expect, it } from 'vitest'

import type { Dashboard } from '@/api/generated'
import {
  cancelEditing,
  dismissConflictNotice,
  editDraft,
  finishEditing,
  hasUnsavedChanges,
  startEditing,
  useEditorStore,
  type SaveDashboard,
  type SaveOutcome,
} from '@/editor/editor-store'
import { fromDraft } from '@/editor/model'

const DASHBOARD: Dashboard = {
  schema_version: 1,
  theme: { preset: 'default', auto_dark_mode: true, touch_scale: 'normal' },
  kiosk: { screensaver_timeout_sec: 60, wake_on: [] },
  pages: [
    {
      id: 'home',
      title: 'Home',
      icon: 'home',
      grid_columns: 4,
      widgets: [{ type: 'light-control', target: 'lamp/main', col_span: 2, row_span: 1 }],
    },
  ],
}

function renamePage(title: string) {
  editDraft({ type: 'editPage', pageId: 'home', title, icon: 'home' })
}

function recordingSave(outcome: SaveOutcome) {
  const calls: { dashboard: Dashboard; revision: string | null }[] = []
  const save: SaveDashboard = (dashboard, revision) => {
    calls.push({ dashboard, revision })
    return Promise.resolve(outcome)
  }
  return { calls, save }
}

beforeEach(() => {
  useEditorStore.setState({ session: null, saving: false, conflictNotice: false })
})

describe('edit session', () => {
  it('edits a draft and saves it against the revision it started from', async () => {
    startEditing(DASHBOARD, 'rev-1')
    expect(hasUnsavedChanges()).toBe(false)
    renamePage('Inicio')
    expect(hasUnsavedChanges()).toBe(true)

    const { calls, save } = recordingSave({ ok: true })
    await expect(finishEditing(save)).resolves.toBe('saved')
    expect(calls).toHaveLength(1)
    expect(calls[0]?.revision).toBe('rev-1')
    expect(calls[0]?.dashboard.pages[0]?.title).toBe('Inicio')
    expect(useEditorStore.getState()).toEqual({ session: null, saving: false, conflictNotice: false })
  })

  it('does not save when nothing changed', async () => {
    startEditing(DASHBOARD, 'rev-1')
    renamePage('Home')
    const { calls, save } = recordingSave({ ok: true })
    await expect(finishEditing(save)).resolves.toBe('unchanged')
    expect(calls).toHaveLength(0)
    expect(useEditorStore.getState().session).toBeNull()
  })

  it('drops the draft when someone else saved first (412)', async () => {
    startEditing(DASHBOARD, 'rev-1')
    renamePage('Mine')
    const { save } = recordingSave({ ok: false, reason: 'conflict' })
    await expect(finishEditing(save)).resolves.toBe('conflict')
    // The screen goes back to showing the (reloaded) saved dashboard, and says why.
    expect(useEditorStore.getState()).toEqual({ session: null, saving: false, conflictNotice: true })
    dismissConflictNotice()
    expect(useEditorStore.getState().conflictNotice).toBe(false)
  })

  it('keeps editing when saving failed, so the person can try again', async () => {
    startEditing(DASHBOARD, 'rev-1')
    renamePage('Mine')
    const failing = recordingSave({ ok: false, reason: 'failed' })
    await expect(finishEditing(failing.save)).resolves.toBe('failed')
    const session = useEditorStore.getState().session
    expect(session?.draft.pages[0]?.title).toBe('Mine')
    expect(useEditorStore.getState().saving).toBe(false)

    const throwing: SaveDashboard = () => Promise.reject(new Error('network'))
    await expect(finishEditing(throwing)).resolves.toBe('failed')
    expect(useEditorStore.getState().session).not.toBeNull()

    const retry = recordingSave({ ok: true })
    await expect(finishEditing(retry.save)).resolves.toBe('saved')
    expect(retry.calls[0]?.dashboard.pages[0]?.title).toBe('Mine')
  })

  it('ignores edits and a second Done while saving', async () => {
    startEditing(DASHBOARD, 'rev-1')
    renamePage('Mine')
    let resolve: (outcome: SaveOutcome) => void = () => undefined
    const slow: SaveDashboard = () =>
      new Promise((done) => {
        resolve = done
      })
    const first = finishEditing(slow)
    expect(useEditorStore.getState().saving).toBe(true)
    renamePage('Changed while saving')
    cancelEditing()
    await expect(finishEditing(slow)).resolves.toBe('busy')
    const session = useEditorStore.getState().session
    expect(session === null ? null : fromDraft(session.draft).pages[0]?.title).toBe('Mine')
    resolve({ ok: true })
    await expect(first).resolves.toBe('saved')
  })

  it('cancel throws the draft away; starting again begins from the saved dashboard', () => {
    startEditing(DASHBOARD, 'rev-1')
    renamePage('Mine')
    cancelEditing()
    expect(useEditorStore.getState().session).toBeNull()
    startEditing(DASHBOARD, 'rev-2')
    expect(useEditorStore.getState().session?.baseRevision).toBe('rev-2')
    expect(hasUnsavedChanges()).toBe(false)
  })

  it('points out the tile that was just moved or added', () => {
    startEditing(DASHBOARD, 'rev-1')
    editDraft({ type: 'addWidget', pageId: 'home', widget: { type: 'clock-weather', col_span: 2, row_span: 1 } })
    const added = useEditorStore.getState().session
    const key = added?.draft.pages[0]?.widgets[1]?.key
    expect(key).toBeDefined()
    expect(added?.highlightKey).toBe(key)
    renamePage('Other')
    expect(useEditorStore.getState().session?.highlightKey).toBeNull()
  })
})
