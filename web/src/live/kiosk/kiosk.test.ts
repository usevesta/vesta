import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { EndpointEvent, Kiosk } from '@/api/generated'
import { IdleTimer } from '@/live/kiosk/idle-timer'
import {
  dismissDoorbell,
  selectScreensaverVisible,
  startDoorbell,
  startScreensaver,
  useKioskStore,
  wakeScreen,
  type EventSource,
} from '@/live/kiosk/kiosk'

class FakeEvents implements EventSource {
  readonly #listeners = new Set<(event: EndpointEvent) => void>()
  onEvent = (listener: (event: EndpointEvent) => void) => {
    this.#listeners.add(listener)
    return () => {
      this.#listeners.delete(listener)
    }
  }
  emit(target: string, event: EndpointEvent['event']) {
    for (const listener of this.#listeners) listener({ target, event, at: '2026-01-01T12:00:00Z' })
  }
}

const KIOSK: Kiosk = {
  screensaver_timeout_sec: 120,
  wake_on: [{ target: 'front_doorbell/main', event: 'occupancy.detected' }],
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('IdleTimer', () => {
  it('goes idle after the timeout and wakes on activity', () => {
    const changes: boolean[] = []
    const timer = new IdleTimer(1_000, (idle) => changes.push(idle))
    timer.start()
    vi.advanceTimersByTime(999)
    timer.activity()
    vi.advanceTimersByTime(999)
    expect(changes).toEqual([])
    vi.advanceTimersByTime(1)
    expect(changes).toEqual([true])
    timer.activity()
    expect(changes).toEqual([true, false])
    timer.stop()
    vi.advanceTimersByTime(5_000)
    expect(changes).toEqual([true, false])
  })

  it('never sleeps with a zero timeout', () => {
    const changes: boolean[] = []
    new IdleTimer(0, (idle) => changes.push(idle)).start()
    vi.advanceTimersByTime(60_000)
    expect(changes).toEqual([])
  })
})

describe('screensaver', () => {
  it('dims after the timeout and wakes on touches, wake events and wakeScreen()', () => {
    const screen = new EventTarget()
    const events = new FakeEvents()
    const stop = startScreensaver(KIOSK, events, screen)
    const asleep = () => useKioskStore.getState().asleep

    vi.advanceTimersByTime(119_000)
    screen.dispatchEvent(new Event('pointerdown'))
    vi.advanceTimersByTime(119_000)
    expect(asleep()).toBe(false)
    vi.advanceTimersByTime(1_000)
    expect(asleep()).toBe(true)

    // While asleep, a touch only wakes through the screensaver itself.
    screen.dispatchEvent(new Event('pointerdown'))
    expect(asleep()).toBe(true)
    wakeScreen()
    expect(asleep()).toBe(false)

    vi.advanceTimersByTime(120_000)
    events.emit('front_doorbell/main', 'doorbell.pressed') // not in wake_on
    expect(asleep()).toBe(true)
    events.emit('front_doorbell/main', 'occupancy.detected')
    expect(asleep()).toBe(false)

    vi.advanceTimersByTime(120_000)
    screen.dispatchEvent(new Event('keydown'))
    expect(asleep()).toBe(false)

    stop()
    vi.advanceTimersByTime(600_000)
    expect(asleep()).toBe(false)
  })
})

describe('doorbell overlay', () => {
  const overlay = { doorbell_target: 'front_doorbell/main', open_target: 'front_door_strike/main', auto_dismiss_sec: 45 }

  it('shows on a press of its doorbell and closes by itself', () => {
    const events = new FakeEvents()
    const stop = startDoorbell(overlay, events, () => 1_000)
    events.emit('other_doorbell/main', 'doorbell.pressed')
    events.emit('front_doorbell/main', 'occupancy.detected')
    expect(useKioskStore.getState().ringing).toBe(false)

    events.emit('front_doorbell/main', 'doorbell.pressed')
    expect(useKioskStore.getState()).toMatchObject({ ringing: true, ringingUntil: 46_000 })

    vi.advanceTimersByTime(30_000)
    // Another press restarts the countdown.
    events.emit('front_doorbell/main', 'doorbell.pressed')
    vi.advanceTimersByTime(44_000)
    expect(useKioskStore.getState().ringing).toBe(true)
    vi.advanceTimersByTime(1_000)
    expect(useKioskStore.getState()).toMatchObject({ ringing: false, ringingUntil: null })

    events.emit('front_doorbell/main', 'doorbell.pressed')
    dismissDoorbell()
    expect(useKioskStore.getState().ringing).toBe(false)
    stop()
  })

  it('keeps the screensaver away while ringing', () => {
    useKioskStore.setState({ asleep: true, ringing: true })
    expect(selectScreensaverVisible(useKioskStore.getState())).toBe(false)
    useKioskStore.setState({ ringing: false })
    expect(selectScreensaverVisible(useKioskStore.getState())).toBe(true)
    useKioskStore.setState({ asleep: false })
  })
})
