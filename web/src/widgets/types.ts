/**
 * Props every widget accepts. Widgets are presentational: they receive
 * plain values and callbacks. Mapping device state and commands (generated
 * API types) onto these props is the integration layer's job.
 */
export interface WidgetStatusProps {
  /** A command was sent and the device has not confirmed it yet. */
  pending?: boolean
  /** The device is not responding: controls are disabled. */
  unavailable?: boolean
}
