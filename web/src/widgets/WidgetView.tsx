import { createElement } from 'react'

import {
  getWidgetComponent,
  isWidgetType,
  type AnyWidgetSpec,
  type WidgetSpec,
  type WidgetType,
} from '@/widgets/registry'
import { UnknownWidget } from '@/widgets/UnknownWidget'

function isKnownSpec(spec: WidgetSpec | AnyWidgetSpec): spec is WidgetSpec {
  return isWidgetType(spec.type)
}

function renderKnown<K extends WidgetType>(spec: WidgetSpec<K>) {
  return createElement(getWidgetComponent(spec.type), spec.props)
}

export interface WidgetViewProps {
  spec: WidgetSpec | AnyWidgetSpec
}

/** Render a widget from the registry, or a friendly placeholder for unknown types. */
export function WidgetView({ spec }: WidgetViewProps) {
  return isKnownSpec(spec) ? renderKnown(spec) : <UnknownWidget type={spec.type} />
}
