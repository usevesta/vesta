/**
 * The physical screen while the panel shows: kept awake, and dimmed while
 * the screensaver is up when the device can do it. Where it can't (a
 * browser, a desktop monitor) the screensaver's dark overlay is all there is.
 */
import { selectScreensaverVisible, useKioskStore } from '@/live/kiosk/kiosk'
import type { Platform } from '@/platform'

/** Backlight level while the screensaver shows. */
export const SCREENSAVER_BRIGHTNESS = 10

type KioskStore = Pick<typeof useKioskStore, 'getState' | 'subscribe'>

/** Returns a stop function that restores brightness and lets the screen sleep again. */
export function startScreenControl(platform: Platform, store: KioskStore = useKioskStore): () => void {
  let stopped = false
  let awake = false
  let dimmed = false
  let unsubscribe = () => {
    // Replaced once the capabilities are known.
  }

  const setDimmed = (dim: boolean) => {
    if (dim === dimmed) return
    dimmed = dim
    if (dim) void platform.setBrightness(SCREENSAVER_BRIGHTNESS)
    else void platform.restoreBrightness()
  }

  void platform.capabilities().then((capabilities) => {
    if (stopped) return
    if (capabilities.keep_awake) {
      awake = true
      void platform.setKeepAwake(true)
    }
    if (!capabilities.brightness) return
    setDimmed(selectScreensaverVisible(store.getState()))
    unsubscribe = store.subscribe((state) => {
      setDimmed(selectScreensaverVisible(state))
    })
  })

  return () => {
    stopped = true
    unsubscribe()
    setDimmed(false)
    if (awake) void platform.setKeepAwake(false)
  }
}
