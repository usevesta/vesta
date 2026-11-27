import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { cn } from '@/lib/utils'

export interface ShellPage {
  id: string
  title: string
  /** Semantic icon name (see src/lib/icons.ts); unknown names get a fallback. */
  icon: string
}

export interface PageNavProps {
  pages: readonly ShellPage[]
  activePageId: string | null
  onSelectPage: (id: string) => void
  className?: string
}

/** Bottom bar on phones, side rail on tablets and larger screens. */
export function PageNav({ pages, activePageId, onSelectPage, className }: PageNavProps) {
  const { t } = useTranslation()
  return (
    <nav
      aria-label={t('nav.label')}
      className={cn(
        'glass-surface shrink-0 border-t bg-card/80 pb-[env(safe-area-inset-bottom)] md:border-t-0 md:border-r md:pb-0 md:pl-[env(safe-area-inset-left)]',
        className,
      )}
    >
      <ul className="flex gap-1 overflow-x-auto p-2 md:h-full md:w-28 md:flex-col md:overflow-x-visible md:overflow-y-auto md:py-4">
        {pages.map((page) => {
          const active = page.id === activePageId
          return (
            <li key={page.id} className="min-w-0 flex-1 md:flex-none">
              <button
                type="button"
                aria-current={active ? 'page' : undefined}
                onClick={() => {
                  onSelectPage(page.id)
                }}
                className={cn(
                  'flex min-h-16 w-full min-w-16 flex-col items-center justify-center gap-1 rounded-xl px-2 py-2 text-sm font-medium transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-20',
                  active
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                )}
              >
                <Icon name={page.icon} className="size-7" />
                <span className="w-full truncate text-center">{page.title}</span>
              </button>
            </li>
          )
        })}
      </ul>
    </nav>
  )
}
