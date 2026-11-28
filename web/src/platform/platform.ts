/**
 * The device the panel runs on: keep the screen awake, dim it, kiosk mode.
 *
 * Inside the VESTA app these are the shell's kiosk commands (the only
 * native calls the page is allowed to make). In a plain browser nothing can
 * be controlled, so every call reports `unsupported` and the panel falls
 * back to what a web page can do. Types are generated from Rust.
 */
import type { BrightnessRequest, KeepAwakeRequest, KioskCapabilities, KioskError } from '@/api/generated/shell'

/** `ok`, or why nothing happened. */
export type PlatformResult = 'ok' | KioskError

export interface Platform {
  capabilities: () => Promise<KioskCapabilities>
  setKeepAwake: (on: boolean) => Promise<PlatformResult>
  /** 0 (darkest) to 100. */
  setBrightness: (percent: number) => Promise<PlatformResult>
  /** Give brightness back to the system and the user's own setting. */
  restoreBrightness: () => Promise<PlatformResult>
  enterKiosk: () => Promise<PlatformResult>
  exitKiosk: () => Promise<PlatformResult>
}

export const NO_CAPABILITIES: KioskCapabilities = Object.freeze({
  brightness: false,
  keep_awake: false,
  kiosk: false,
  locks_device: false,
})

const unsupported = (): Promise<PlatformResult> => Promise.resolve('unsupported')

/** A plain browser: nothing to control. */
export const browserPlatform: Platform = Object.freeze({
  capabilities: () => Promise.resolve(NO_CAPABILITIES),
  setKeepAwake: unsupported,
  setBrightness: unsupported,
  restoreBrightness: unsupported,
  enterKiosk: unsupported,
  exitKiosk: unsupported,
})

/** Tauri's `invoke`, narrowed to what this module needs. */
export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>

function isKioskError(value: unknown): value is KioskError {
  return value === 'unsupported' || value === 'failed'
}

function decodeCapabilities(value: unknown): KioskCapabilities {
  if (typeof value !== 'object' || value === null) return NO_CAPABILITIES
  const flag = (key: keyof KioskCapabilities) => (value as Record<string, unknown>)[key] === true
  return {
    brightness: flag('brightness'),
    keep_awake: flag('keep_awake'),
    kiosk: flag('kiosk'),
    locks_device: flag('locks_device'),
  }
}

function clampPercent(percent: number): number {
  return Number.isFinite(percent) ? Math.min(100, Math.max(0, Math.round(percent))) : 100
}

/** The VESTA app shell, through its kiosk commands. */
export function shellPlatform(invoke: Invoke): Platform {
  const run = async (command: string, args?: Record<string, unknown>): Promise<PlatformResult> => {
    try {
      await invoke<null>(command, args)
      return 'ok'
    } catch (error) {
      return isKioskError(error) ? error : 'failed'
    }
  }
  return {
    capabilities: async () => {
      try {
        return decodeCapabilities(await invoke<unknown>('kiosk_capabilities'))
      } catch {
        return NO_CAPABILITIES
      }
    },
    setKeepAwake: (on) => {
      const request: KeepAwakeRequest = { on }
      return run('kiosk_set_keep_awake', { request })
    },
    setBrightness: (percent) => {
      const request: BrightnessRequest = { percent: clampPercent(percent) }
      return run('kiosk_set_brightness', { request })
    },
    restoreBrightness: () => run('kiosk_restore_brightness'),
    enterKiosk: () => run('kiosk_enter'),
    exitKiosk: () => run('kiosk_exit'),
  }
}
