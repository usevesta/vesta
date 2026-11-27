import { useTranslation } from 'react-i18next'

import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface UnknownWidgetProps {
  /** The unrecognised widget type. Kept out of the visible text on purpose. */
  type: string
}

/** Friendly placeholder for widget types this version of the app does not know. */
export function UnknownWidget({ type }: UnknownWidgetProps) {
  const { t } = useTranslation()
  return (
    <div className="h-full" data-widget-type={type}>
      <WidgetFrame icon="unknown" title={t('unknownWidget.title')} subtitle={t('unknownWidget.body')} />
    </div>
  )
}
