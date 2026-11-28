import type { IconName } from '@/lib/icons'

/** Icons a page can have (they show in the page list on the side). */
export const PAGE_ICONS = [
  'home',
  'living_room',
  'bedroom',
  'kitchen',
  'bathroom',
  'garden',
  'garage',
  'light',
  'blind',
  'door',
  'camera',
  'fan',
] as const satisfies readonly IconName[]

export type PageIcon = (typeof PAGE_ICONS)[number]

export function isPageIcon(name: string | undefined): name is PageIcon {
  return (PAGE_ICONS as readonly (string | undefined)[]).includes(name)
}
