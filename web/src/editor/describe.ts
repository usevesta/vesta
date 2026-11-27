import { useTranslation } from 'react-i18next'

import type { Widget } from '@/api/generated'
import { widgetLabel, type HomeItem } from '@/editor/model'
import { isIconName, type IconName } from '@/lib/icons'
import { isWidgetType, widgetMeta } from '@/widgets/registry'

export interface TileDescription {
  /** What the tile is called on the screen. */
  readonly name: string
  /** The name it shows without a custom one; undefined when it shows none (the clock). */
  readonly ownName: string | undefined
  /** Second line: the original name under a custom one, or why it cannot be found. */
  readonly detail?: string
  readonly icon: IconName
}

/** How the editor names a tile, without ever showing ids or types. */
export function useDescribeTile(): (widget: Widget, item: HomeItem | undefined, itemsReady: boolean) => TileDescription {
  const { t } = useTranslation()
  return (widget, item, itemsReady) => {
    const label = widgetLabel(widget)
    if (!isWidgetType(widget.type)) {
      return { name: label ?? t('unknownWidget.title'), ownName: undefined, icon: 'unknown' }
    }
    const meta = widgetMeta[widget.type]
    if (widget.target === undefined) {
      // A tile that shows no device: the clock. Its label is a place name.
      return { name: t('editor.clock'), ownName: undefined, detail: label, icon: meta.icon }
    }
    const missing = item === undefined && itemsReady
    let detail: string | undefined
    if (missing) detail = t('editor.notFound')
    else if (label !== undefined && item !== undefined && label !== item.name) detail = item.name
    return {
      name: label ?? item?.name ?? t('editor.unnamed'),
      ownName: item?.name,
      detail,
      icon: missing ? 'warning' : item !== undefined && isIconName(item.type) ? item.type : meta.icon,
    }
  }
}
