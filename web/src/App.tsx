import { useTranslation } from 'react-i18next'

import { LiveApp } from '@/app/LiveApp'
import { Toaster } from '@/components/ui/sonner'
import { DemoApp } from '@/demo/DemoApp'
import { isDemoMode } from '@/demo/demo-mode'

const DEMO = isDemoMode()

export default function App() {
  const { t } = useTranslation()
  return (
    <>
      {DEMO ? <DemoApp /> : <LiveApp />}
      <Toaster
        position="top-center"
        closeButton={false}
        containerAriaLabel={t('common.notifications')}
        toastOptions={{ duration: 4_000 }}
      />
    </>
  )
}
