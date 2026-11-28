import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useKioskStore } from '@/live/kiosk/kiosk'
import { SCREENSAVER_BRIGHTNESS, startScreenControl } from '@/live/kiosk/screen'
import { browserPlatform, NO_CAPABILITIES, platform, shellPlatform, type Platform } from '@/platform'
import type { Invoke } from '@/platform/platform'

describe('platform()', () => {
  it('is a plain browser outside the app, where nothing is supported', async () => {
    expect(platform()).toBe(browserPlatform)
    const device = platform()
    await expect(device.capabilities()).resolves.toEqual(NO_CAPABILITIES)
    await expect(device.setKeepAwake(true)).resolves.toBe('unsupported')
    await expect(device.setBrightness(10)).resolves.toBe('unsupported')
    await expect(device.restoreBrightness()).resolves.toBe('unsupported')
    await expect(device.enterKiosk()).resolves.toBe('unsupported')
    await expect(device.exitKiosk()).resolves.toBe('unsupported')
  })
})

function fakeInvoke(reply: (command: string, args?: Record<string, unknown>) => unknown) {
  const calls: [string, Record<string, unknown> | undefined][] = []
  const invoke: Invoke = <T>(command: string, args?: Record<string, unknown>) => {
    calls.push([command, args])
    try {
      return Promise.resolve(reply(command, args) as T)
    } catch (error) {
      // Tauri rejects with whatever the command returned as its error.
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      return Promise.reject(error)
    }
  }
  return { invoke, calls }
}

describe('shellPlatform', () => {
  it('calls the kiosk commands with the payloads Rust expects', async () => {
    const { invoke, calls } = fakeInvoke(() => null)
    const shell = shellPlatform(invoke)
    await expect(shell.setKeepAwake(true)).resolves.toBe('ok')
    await expect(shell.setBrightness(250)).resolves.toBe('ok')
    await expect(shell.setBrightness(12.4)).resolves.toBe('ok')
    await expect(shell.restoreBrightness()).resolves.toBe('ok')
    await expect(shell.enterKiosk()).resolves.toBe('ok')
    await expect(shell.exitKiosk()).resolves.toBe('ok')
    expect(calls).toEqual([
      ['kiosk_set_keep_awake', { request: { on: true } }],
      ['kiosk_set_brightness', { request: { percent: 100 } }],
      ['kiosk_set_brightness', { request: { percent: 12 } }],
      ['kiosk_restore_brightness', undefined],
      ['kiosk_enter', undefined],
      ['kiosk_exit', undefined],
    ])
  })

  it('reports refusals instead of throwing', async () => {
    const reasons: unknown[] = ['unsupported', 'failed', new Error('not allowed by ACL')]
    const { invoke } = fakeInvoke(() => {
      throw reasons.shift()
    })
    const shell = shellPlatform(invoke)
    await expect(shell.setBrightness(10)).resolves.toBe('unsupported')
    await expect(shell.setBrightness(10)).resolves.toBe('failed')
    await expect(shell.enterKiosk()).resolves.toBe('failed')
  })

  it('reads capabilities defensively', async () => {
    const replies: unknown[] = [
      { brightness: true, keep_awake: true, kiosk: false, locks_device: false },
      { brightness: 'yes' },
      null,
    ]
    const { invoke } = fakeInvoke(() => replies.shift())
    const shell = shellPlatform(invoke)
    await expect(shell.capabilities()).resolves.toEqual({
      brightness: true,
      keep_awake: true,
      kiosk: false,
      locks_device: false,
    })
    await expect(shell.capabilities()).resolves.toEqual(NO_CAPABILITIES)
    await expect(shell.capabilities()).resolves.toEqual(NO_CAPABILITIES)
    const failing = shellPlatform(() => Promise.reject(new Error('no bridge')))
    await expect(failing.capabilities()).resolves.toEqual(NO_CAPABILITIES)
  })
})

class FakeDevice implements Platform {
  readonly calls: string[] = []
  readonly brightness: boolean
  constructor(brightness: boolean) {
    this.brightness = brightness
  }
  capabilities = () =>
    Promise.resolve({ ...NO_CAPABILITIES, brightness: this.brightness, keep_awake: true })
  setKeepAwake = (on: boolean) => this.#record(`keep_awake:${String(on)}`)
  setBrightness = (percent: number) => this.#record(`brightness:${String(percent)}`)
  restoreBrightness = () => this.#record('restore')
  enterKiosk = () => this.#record('enter')
  exitKiosk = () => this.#record('exit')
  #record(call: string) {
    this.calls.push(call)
    return Promise.resolve('ok' as const)
  }
}

describe('startScreenControl', () => {
  beforeEach(() => {
    useKioskStore.setState({ asleep: false, ringing: false, ringingUntil: null })
  })
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('keeps the screen awake and dims it while the screensaver shows', async () => {
    const device = new FakeDevice(true)
    const stop = startScreenControl(device)
    await vi.waitFor(() => {
      expect(device.calls).toEqual(['keep_awake:true'])
    })

    useKioskStore.setState({ asleep: true })
    expect(device.calls.at(-1)).toBe(`brightness:${String(SCREENSAVER_BRIGHTNESS)}`)
    // The doorbell wakes the screen even while asleep.
    useKioskStore.setState({ ringing: true })
    expect(device.calls.at(-1)).toBe('restore')
    useKioskStore.setState({ ringing: false, asleep: false })
    expect(device.calls.filter((call) => call === 'restore')).toHaveLength(1)

    useKioskStore.setState({ asleep: true })
    stop()
    expect(device.calls.slice(-2)).toEqual(['restore', 'keep_awake:false'])
    useKioskStore.setState({ asleep: false })
    expect(device.calls.at(-1)).toBe('keep_awake:false')
  })

  it('only keeps the screen awake where it cannot dim', async () => {
    const device = new FakeDevice(false)
    const stop = startScreenControl(device)
    await vi.waitFor(() => {
      expect(device.calls).toEqual(['keep_awake:true'])
    })
    useKioskStore.setState({ asleep: true })
    stop()
    expect(device.calls).toEqual(['keep_awake:true', 'keep_awake:false'])
  })

  it('does nothing in a plain browser', async () => {
    const stop = startScreenControl(browserPlatform)
    await Promise.resolve()
    useKioskStore.setState({ asleep: true })
    stop()
    expect(useKioskStore.getState().asleep).toBe(true)
  })
})
