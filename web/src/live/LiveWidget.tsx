import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import type { Widget } from '@/api/generated'
import { bindWidget, type BindingContext } from '@/live/bindings'
import { useEndpointView, useRelatedEndpointViews } from '@/live/device-store'
import { liveClient } from '@/live/live'
import { WidgetView } from '@/widgets/WidgetView'

/** One dashboard widget, bound to its endpoint's live state. */
export function LiveWidget({ widget }: { widget: Widget }) {
  const { t } = useTranslation()
  const view = useEndpointView(widget.target)
  // Only a charger tile shows other parts of its device (its meters).
  const related = useRelatedEndpointViews(widget.type === 'ev-charger' ? widget.target : undefined)
  const context: BindingContext = {
    send: (target, command, options) => liveClient().sendCommand(target, command, options),
    fallbackLabel: t('widget.unknownDevice'),
    onOpened: (label) => {
      toast.success(t('door.opened', { name: label }))
    },
  }
  return <WidgetView spec={bindWidget(widget, view, context, related)} />
}
