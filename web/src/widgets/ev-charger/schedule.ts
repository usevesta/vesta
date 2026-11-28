/**
 * Pure helpers for a charger's charging hours: what to say about them in
 * plain words and whether edited hours fit the charger. The home checks
 * again before anything reaches the charger; this only keeps Save honest.
 * No React, so it is unit tested.
 */
import type { ChargingScheduleFeatures, ChargingWindow, TimeOfDay, Weekday } from '@/api/generated'

/** Monday first, like the charger and the capability model. */
export const WEEK: readonly Weekday[] = [
  'monday',
  'tuesday',
  'wednesday',
  'thursday',
  'friday',
  'saturday',
  'sunday',
]

const WORKDAYS: readonly Weekday[] = WEEK.slice(0, 5)
const WEEKEND: readonly Weekday[] = WEEK.slice(5)

/** A new window: the usual cheap night hours, every day. */
export function newWindow(): ChargingWindow {
  return { start: '00:00', end: '08:00', days: [...WEEK] }
}

/** Days in week order, each once. */
export function sortDays(days: readonly Weekday[]): Weekday[] {
  return WEEK.filter((day) => days.includes(day))
}

export function toggleDay(days: readonly Weekday[], day: Weekday): Weekday[] {
  return sortDays(days.includes(day) ? days.filter((d) => d !== day) : [...days, day])
}

/** Minutes past midnight, or null when it is not `HH:MM`. */
export function minutesOf(time: TimeOfDay): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(time)
  if (match === null) return null
  const hours = Number(match[1])
  const minutes = Number(match[2])
  return hours < 24 && minutes < 60 ? hours * 60 + minutes : null
}

/** Ends the next day (e.g. 22:00–06:00). */
export function isOvernight(window: ChargingWindow): boolean {
  const start = minutesOf(window.start)
  const end = minutesOf(window.end)
  return start !== null && end !== null && end < start
}

/** Why edited hours cannot be saved, as a translation key. */
export type ScheduleProblem =
  | 'ev.hours.problem.noDays'
  | 'ev.hours.problem.sameTime'
  | 'ev.hours.problem.overnight'
  | 'ev.hours.problem.time'
  | 'ev.hours.problem.tooMany'
  | 'ev.hours.problem.none'

/** The first problem with `windows` on this charger, or null when they fit. */
export function scheduleProblem(
  windows: readonly ChargingWindow[],
  features: ChargingScheduleFeatures,
): ScheduleProblem | null {
  if (windows.length === 0) return 'ev.hours.problem.none'
  if (windows.length > features.max_windows) return 'ev.hours.problem.tooMany'
  for (const window of windows) {
    const start = minutesOf(window.start)
    const end = minutesOf(window.end)
    if (start === null || end === null) return 'ev.hours.problem.time'
    if (window.days.length === 0) return 'ev.hours.problem.noDays'
    if (start === end) return 'ev.hours.problem.sameTime'
    if (end < start && !features.overnight) return 'ev.hours.problem.overnight'
  }
  return null
}

/** How the days of a window are said. */
export type DaysSummary =
  | { readonly key: 'ev.hours.everyDay' | 'ev.hours.workdays' | 'ev.hours.weekend' }
  | { readonly key: 'ev.hours.someDays'; readonly days: readonly Weekday[] }

export function daysSummary(days: readonly Weekday[]): DaysSummary {
  const sorted = sortDays(days)
  const same = (other: readonly Weekday[]) =>
    sorted.length === other.length && other.every((day) => sorted.includes(day))
  if (same(WEEK)) return { key: 'ev.hours.everyDay' }
  if (same(WORKDAYS)) return { key: 'ev.hours.workdays' }
  if (same(WEEKEND)) return { key: 'ev.hours.weekend' }
  return { key: 'ev.hours.someDays', days: sorted }
}

/** A time as the screen's language writes it ("22:00", "10:00 PM"). */
export function formatTime(time: TimeOfDay, language: string): string {
  const minutes = minutesOf(time)
  if (minutes === null) return time
  const date = new Date(Date.UTC(2026, 0, 5, Math.floor(minutes / 60), minutes % 60))
  return new Intl.DateTimeFormat(language, { hour: 'numeric', minute: '2-digit', timeZone: 'UTC' }).format(date)
}

/** Identical windows once (a charger may hold the same hours twice). */
export function uniqueWindows(windows: readonly ChargingWindow[]): ChargingWindow[] {
  const seen = new Set<string>()
  return windows.filter((window) => {
    const key = `${window.start}-${window.end}-${sortDays(window.days).join(',')}`
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}
