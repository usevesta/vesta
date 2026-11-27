import { lazy, Suspense } from 'react'
import { useTranslation } from 'react-i18next'

import { LiveApp } from '@/app/LiveApp'
import { Toaster } from '@/components/ui/sonner'
import { isDemoMode } from '@/demo/demo-mode'

const DEMO = isDemoMode()

/** The sample home is only loaded with `?demo=1`. */
const DemoApp = lazy(() => import('@/demo/DemoApp').then((module) => ({ default: module.DemoApp })))

export default function App() {
  const { t } = useTranslation()
  return (
    <>
      {DEMO ? (
        <Suspense fallback={null}>
          <DemoApp />
        </Suspense>
      ) : (
        <LiveApp />
      )}
      <Toaster
        position="top-center"
        closeButton={false}
        containerAriaLabel={t('common.notifications')}
        toastOptions={{ duration: 4_000 }}
      />
    </>
  )
}
