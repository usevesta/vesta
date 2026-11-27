import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { DisplaySettings } from '@/components/shell/DisplaySettings'
import { PageNav, type ShellPage } from '@/components/shell/PageNav'

export interface AppShellProps {
  pages: readonly ShellPage[]
  activePageId: string | null
  onSelectPage: (id: string) => void
  /** Shown above the content (connection state, demo notice). */
  banner?: ReactNode
  /** Extra items at the end of the header, before display settings. */
  headerExtra?: ReactNode
  children?: ReactNode
}

/**
 * Full-screen wall panel layout. Page navigation is a side rail on tablets
 * and a bottom bar on phones; only the content area scrolls.
 */
export function AppShell({ pages, activePageId, onSelectPage, banner, headerExtra, children }: AppShellProps) {
  const { t } = useTranslation()
  const activePage = pages.find((page) => page.id === activePageId)

  return (
    <div className="flex h-dvh w-full flex-col overflow-hidden select-none md:flex-row">
      {pages.length > 1 && (
        <PageNav
          pages={pages}
          activePageId={activePageId}
          onSelectPage={onSelectPage}
          className="order-last md:order-first"
        />
      )}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col pt-[env(safe-area-inset-top)] pr-[env(safe-area-inset-right)]">
        <header className="flex min-h-20 flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 md:px-6">
          <h1 className="min-w-32 flex-1 truncate text-2xl font-semibold tracking-tight md:text-3xl">
            {activePage?.title ?? t('app.name')}
          </h1>
          {headerExtra}
          <DisplaySettings />
        </header>
        {banner}
        <main className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pb-4 md:px-6 md:pb-6">
          {children}
        </main>
      </div>
    </div>
  )
}
