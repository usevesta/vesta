/**
 * Wall-panel behaviour: the screensaver (dim overlay after
 * `kiosk.screensaver_timeout_sec` without touches) and the doorbell
 * overlay (`doorbell_overlay`). State lives in a small store so the React
 * parts only render it; the timers live here and are testable.
 */
import { create } from 'zustand'

import type { DoorbellOverlay, EndpointEvent, Kiosk } from '@/api/generated'
import { IdleTimer } from '@/live/kiosk/idle-timer'

export interface KioskState {
  /** No touches for the configured time. */
  asleep: boolean
  /** The doorbell overlay is showing (keeps the screen awake). */
  ringing: boolean
  /** When the doorbell overlay closes by itself (epoch ms). */
  ringingUntil: number | null
}

export const useKioskStore = create<KioskState>()(() => ({ asleep: false, ringing: false, ringingUntil: null }))

/** The screensaver shows while asleep, unless the doorbell is ringing. */
export function selectScreensaverVisible(state: KioskState): boolean {
  return state.asleep && !state.ringing
}

export interface EventSource {
  onEvent: (listener: (event: EndpointEvent) => void) => () => void
}

/** Pointer and keyboard activity that counts as "someone is using the screen". */
const ACTIVITY_EVENTS = ['pointerdown', 'keydown', 'wheel', 'touchstart'] as const

const wakeListeners = new Set<() => void>()

/** Wake the screen now (screensaver tapped, doorbell rang). */
export function wakeScreen(): void {
  for (const listener of [...wakeListeners]) listener()
}

/**
 * Start the screensaver logic. Wakes on activity, on `wakeScreen()`, and
 * on any event listed in `kiosk.wake_on`. Returns a stop function.
 */
export function startScreensaver(kiosk: Kiosk, events: EventSource, target: EventTarget = document): () => void {
  const timer = new IdleTimer(kiosk.screensaver_timeout_sec * 1_000, (asleep) => {
    useKioskStore.setState({ asleep })
  })

  const onActivity = (event: Event) => {
    // While asleep, only the screensaver's own tap (or a key) wakes the
    // screen, so that first touch never reaches a control underneath.
    if (useKioskStore.getState().asleep && event.type !== 'keydown') return
    timer.activity()
  }
  for (const type of ACTIVITY_EVENTS) target.addEventListener(type, onActivity, { capture: true, passive: true })

  const wake = () => {
    timer.activity()
  }
  wakeListeners.add(wake)

  const stopEvents = events.onEvent((event) => {
    if (kiosk.wake_on.some((ref) => ref.target === event.target && ref.event === event.event)) timer.activity()
  })

  timer.start()
  return () => {
    stopEvents()
    wakeListeners.delete(wake)
    for (const type of ACTIVITY_EVENTS) target.removeEventListener(type, onActivity, { capture: true })
    timer.stop()
    useKioskStore.setState({ asleep: false })
  }
}

let dismissTimer: ReturnType<typeof setTimeout> | null = null

function clearDismissTimer(): void {
  if (dismissTimer !== null) clearTimeout(dismissTimer)
  dismissTimer = null
}

/** Close the doorbell overlay (button, auto-dismiss, door opened). */
export function dismissDoorbell(): void {
  clearDismissTimer()
  if (useKioskStore.getState().ringing) {
    useKioskStore.setState({ ringing: false, ringingUntil: null })
    // Count the ring as activity, so the screensaver does not drop in at once.
    wakeScreen()
  }
}

/**
 * Show the doorbell overlay on `doorbell.pressed` from
 * `overlay.doorbell_target`; close it after `auto_dismiss_sec`. Another
 * press while showing restarts the countdown. Returns a stop function.
 */
export function startDoorbell(overlay: DoorbellOverlay, events: EventSource, now: () => number = Date.now): () => void {
  const stopEvents = events.onEvent((event) => {
    if (event.event !== 'doorbell.pressed' || event.target !== overlay.doorbell_target) return
    clearDismissTimer()
    const durationMs = overlay.auto_dismiss_sec * 1_000
    useKioskStore.setState({ ringing: true, ringingUntil: durationMs > 0 ? now() + durationMs : null })
    wakeScreen()
    if (durationMs > 0) dismissTimer = setTimeout(dismissDoorbell, durationMs)
  })
  return () => {
    stopEvents()
    clearDismissTimer()
    useKioskStore.setState({ ringing: false, ringingUntil: null })
  }
}
