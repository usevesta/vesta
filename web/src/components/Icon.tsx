import type { LucideProps } from 'lucide-react'
import { createElement } from 'react'

import { resolveIcon, type IconName } from '@/lib/icons'

export interface IconProps extends Omit<LucideProps, 'ref'> {
  /**
   * A semantic icon name. Known names are type-checked; any other string
   * (e.g. a page icon read from the home's settings) renders the fallback.
   */
  name: IconName | (string & Record<never, never>)
}

/** Renders an icon from the semantic registry (`src/lib/icons.ts`). */
export function Icon({ name, 'aria-hidden': ariaHidden = true, ...props }: IconProps) {
  // Registry components are module-level constants, so this is a stable type.
  return createElement(resolveIcon(name), { 'aria-hidden': ariaHidden, ...props })
}
