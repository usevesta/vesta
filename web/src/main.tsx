import './index.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import App from '@/App'
import { initI18n } from '@/i18n'
import { startThemeSync } from '@/theme/theme-sync'

initI18n()
startThemeSync()

const root = document.getElementById('root')
if (root === null) throw new Error('Missing #root element in index.html')

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
