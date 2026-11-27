import { invoke, isTauri } from '@tauri-apps/api/core'

import { browserPlatform, shellPlatform, type Platform } from '@/platform/platform'

export {
  browserPlatform,
  NO_CAPABILITIES,
  shellPlatform,
  type Platform,
  type PlatformResult,
} from '@/platform/platform'

let current: Platform | null = null

/** This device: the app shell when running inside it, a plain browser otherwise. */
export function platform(): Platform {
  current ??= isTauri() ? shellPlatform(invoke) : browserPlatform
  return current
}
