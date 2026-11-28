import { useLayoutEffect, useRef, useState, type RefObject } from 'react'

/**
 * Width of the returned ref's element, kept up to date with a
 * ResizeObserver (null until measured, or where there is no observer).
 * Widgets use it to pick a layout for the tile size they were given.
 */
export function useElementWidth<T extends HTMLElement = HTMLDivElement>(): [RefObject<T | null>, number | null] {
  const ref = useRef<T | null>(null)
  const [width, setWidth] = useState<number | null>(null)
  useLayoutEffect(() => {
    const element = ref.current
    if (element === null || typeof ResizeObserver === 'undefined') return
    // Before the first paint, so a tile never flashes the wrong layout.
    setWidth(element.getBoundingClientRect().width)
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (entry !== undefined) setWidth(Math.round(entry.contentRect.width))
    })
    observer.observe(element)
    return () => {
      observer.disconnect()
    }
  }, [])
  return [ref, width]
}
