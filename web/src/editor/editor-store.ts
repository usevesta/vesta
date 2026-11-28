/**
 * The edit session of this screen: the draft being edited, the revision it
 * started from and whether it is being saved. The screen keeps showing the
 * saved dashboard everywhere else (kiosk, other screens) until "Done".
 *
 * Saving goes through a `SaveDashboard` function, so the same editor works
 * against the home (`src/live/`) and in demo mode (in memory).
 */
import { create } from 'zustand'

import type { Dashboard } from '@/api/generated'
import { applyEdit, fromDraft, isDirty, toDraft, type Draft, type EditAction } from '@/editor/model'

export type SaveOutcome = { readonly ok: true } | { readonly ok: false; readonly reason: 'conflict' | 'failed' }

/** Store `dashboard`, edited from `baseRevision`. `conflict`: someone else saved first. */
export type SaveDashboard = (dashboard: Dashboard, baseRevision: string | null) => Promise<SaveOutcome>

export interface EditSession {
  readonly draft: Draft
  /** Revision of the dashboard the edit started from (sent as `If-Match`). */
  readonly baseRevision: string | null
  /** Tile last moved, resized or added, so the screen can point it out. */
  readonly highlightKey: string | null
}

export interface EditorState {
  /** Null while the screen is not being edited. */
  readonly session: EditSession | null
  readonly saving: boolean
  /** A save was refused because someone else saved first: the screen says so once. */
  readonly conflictNotice: boolean
}

export const useEditorStore = create<EditorState>()(() => ({ session: null, saving: false, conflictNotice: false }))

export function startEditing(dashboard: Dashboard, revision: string | null): void {
  if (useEditorStore.getState().session !== null) return
  useEditorStore.setState({ session: { draft: toDraft(dashboard), baseRevision: revision, highlightKey: null } })
}

function highlightFor(action: EditAction, before: Draft, after: Draft): string | null {
  switch (action.type) {
    case 'moveWidget':
    case 'resizeWidget':
      return action.key
    case 'restoreWidget':
      return action.widget.key
    case 'addWidget':
      return after.nextKey === before.nextKey ? null : `w${String(before.nextKey)}`
    default:
      return null
  }
}

/** Apply one edit to the draft. Ignored while saving or when not editing. */
export function editDraft(action: EditAction): void {
  const { session, saving } = useEditorStore.getState()
  if (session === null || saving) return
  const draft = applyEdit(session.draft, action)
  if (draft === session.draft) return
  useEditorStore.setState({ session: { ...session, draft, highlightKey: highlightFor(action, session.draft, draft) } })
}

/** The draft has changes that "Cancel" would throw away. */
export function hasUnsavedChanges(state: EditorState = useEditorStore.getState()): boolean {
  return state.session !== null && isDirty(state.session.draft)
}

/** The person read the conflict notice. */
export function dismissConflictNotice(): void {
  useEditorStore.setState({ conflictNotice: false })
}

/** Leave edit mode without saving. */
export function cancelEditing(): void {
  if (useEditorStore.getState().saving) return
  useEditorStore.setState({ session: null })
}

/**
 * - `saved`: stored; the screen shows it.
 * - `unchanged`: nothing to save; edit mode ends.
 * - `conflict`: someone else saved first; the draft is dropped and the
 *   screen shows the latest version.
 * - `failed`: could not save; still editing, the person can try again.
 * - `busy`: a save is already running (double tap).
 */
export type FinishOutcome = 'saved' | 'unchanged' | 'conflict' | 'failed' | 'busy'

/** "Done": save the draft if it changed and leave edit mode. */
export async function finishEditing(save: SaveDashboard): Promise<FinishOutcome> {
  const { session, saving } = useEditorStore.getState()
  if (saving) return 'busy'
  if (session === null) return 'unchanged'
  if (!isDirty(session.draft)) {
    useEditorStore.setState({ session: null })
    return 'unchanged'
  }

  useEditorStore.setState({ saving: true })
  let outcome: SaveOutcome
  try {
    outcome = await save(fromDraft(session.draft), session.baseRevision)
  } catch {
    outcome = { ok: false, reason: 'failed' }
  }
  if (outcome.ok) {
    useEditorStore.setState({ session: null, saving: false })
    return 'saved'
  }
  if (outcome.reason === 'conflict') {
    useEditorStore.setState({ session: null, saving: false, conflictNotice: true })
    return 'conflict'
  }
  useEditorStore.setState({ saving: false })
  return 'failed'
}
