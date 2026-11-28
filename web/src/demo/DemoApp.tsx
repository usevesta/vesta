import { useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import type { Widget } from '@/api/generated'
import { DashboardScreen } from '@/app/DashboardScreen'
import { Icon } from '@/components/Icon'
import { Badge } from '@/components/ui/badge'
import {
  DEMO_ENTRY_BY_TARGET,
  demoDashboard,
  demoItems,
  demoName,
  saveDemoDashboard,
  useDemoDashboardStore,
  type DemoThing,
} from '@/demo/demo-home'
import {
  changeDemoCharger,
  DEMO_CHARGER_MODES,
  DEMO_CURRENT_RANGE,
  startDemoCharger,
  useDemoCharger,
  type DemoCharger,
} from '@/demo/demo-charger'
import { useDemoStore, type DemoState } from '@/demo/demo-store'
import { widgetLabel } from '@/editor/model'
import { isWidgetType, type AnyWidgetSpec, type WidgetSpec } from '@/widgets/registry'
import { UnknownWidget } from '@/widgets/UnknownWidget'
import { WidgetView } from '@/widgets/WidgetView'

/** Presentational props for one sample thing, from the mock state. */
function demoSpec(
  thing: DemoThing,
  label: string,
  demo: DemoState,
  charger: DemoCharger,
  rows: number,
  run: (name: string, command: Promise<boolean>) => void,
  onDoorOpened: (name: string) => void,
): WidgetSpec {
  switch (thing.kind) {
    case 'light': {
      const { id } = thing
      const state = demo.lights[id]
      return {
        type: 'light-control',
        props: {
          label,
          on: state.on,
          brightness: state.brightness,
          colorTemperature: state.colorTemperature,
          pending: state.pending,
          unavailable: state.unavailable,
          onToggle: (on) => {
            run(label, demo.setLight(id, { on }))
          },
          onBrightnessChange: (brightness) => {
            // Dimming a light that is off switches it on.
            run(label, demo.setLight(id, { brightness, on: true }))
          },
          onColorTemperatureChange: (kelvin) => {
            const current = state.colorTemperature
            if (current === undefined) return
            run(label, demo.setLight(id, { colorTemperature: { ...current, kelvin }, on: true }))
          },
        },
      }
    }
    case 'fan': {
      const { id } = thing
      const fan = demo.fans[id]
      return {
        type: 'fan-control',
        props: {
          label,
          on: fan.on,
          speed: fan.speed,
          speedSteps: fan.speedSteps,
          direction: fan.direction,
          pending: fan.pending,
          onToggle: (on) => {
            run(label, demo.setFan(id, { on }))
          },
          onSpeedChange: (speed) => {
            run(label, demo.setFan(id, { speed, on: true }))
          },
          onDirectionChange: (direction) => {
            run(label, demo.setFan(id, { direction }))
          },
        },
      }
    }
    case 'cover': {
      const { id } = thing
      const state = demo.covers[id]
      return {
        type: 'cover-control',
        props: {
          label,
          icon: thing.icon,
          sensitive: thing.sensitive,
          position: state.position,
          isOpen: state.isOpen,
          motion: state.motion,
          pending: state.pending,
          onOpen: () => {
            demo.moveCover(id, 'open')
          },
          onClose: () => {
            demo.moveCover(id, 'close')
          },
          onStop: () => {
            demo.moveCover(id, 'stop')
          },
          onPositionChange:
            state.position === undefined
              ? undefined
              : (position) => {
                  demo.moveCover(id, position)
                },
        },
      }
    }
    case 'door': {
      const { id } = thing
      return {
        type: 'door-button',
        props: {
          label,
          pending: demo.doors[id].pending,
          onOpen: () => {
            void demo.openDoor(id).then((ok) => {
              if (ok) onDoorOpened(label)
            })
          },
        },
      }
    }
    case 'camera':
      return { type: 'camera-preview', props: { label, unavailable: thing.unavailable } }
    case 'ev':
      return {
        type: 'ev-charger',
        props: {
          label,
          rows,
          pending: charger.pending,
          status: charger.status,
          waitingFor: charger.waitingFor,
          powerW: charger.powerW,
          sessionEnergyWh: charger.sessionEnergyWh,
          sessionDurationS: charger.sessionDurationS,
          currentA: charger.currentA,
          measuredA: charger.measuredA,
          currentRange: DEMO_CURRENT_RANGE,
          autoCurrent: false,
          mode: charger.mode,
          modes: DEMO_CHARGER_MODES,
          locked: charger.locked,
          scheduleEnabled: charger.scheduleEnabled,
          readings: charger.readings,
          onPause: () => {
            run(label, changeDemoCharger({ paused: true }))
          },
          onResume: () => {
            run(label, changeDemoCharger({ paused: false }))
          },
          onCurrentChange: (amps) => {
            run(label, changeDemoCharger({ currentA: amps }))
          },
          onModeChange: (mode) => {
            run(label, changeDemoCharger({ mode }))
          },
          onLockChange: (locked) => {
            run(label, changeDemoCharger({ locked }))
          },
          onScheduleChange: (enabled) => {
            run(label, changeDemoCharger({ scheduleEnabled: enabled }))
          },
        },
      }
  }
}

/** One tile of the sample home. */
function DemoWidget({ widget }: { widget: Widget }) {
  const { t } = useTranslation()
  const demo = useDemoStore()
  const charger = useDemoCharger()

  if (!isWidgetType(widget.type)) {
    const unknown: AnyWidgetSpec = { type: widget.type, props: widget.props }
    return <WidgetView spec={unknown} />
  }
  if (widget.type === 'clock-weather') {
    return <WidgetView spec={{ type: 'clock-weather', props: { label: widgetLabel(widget) } }} />
  }
  const entry = widget.target === undefined ? undefined : DEMO_ENTRY_BY_TARGET.get(widget.target)
  if (entry === undefined) return <UnknownWidget type={widget.type} />

  const label = widgetLabel(widget) ?? demoName(entry, t)
  const spec = demoSpec(
    entry.thing,
    label,
    demo,
    charger,
    widget.row_span,
    (name, command) => {
      void command.then((ok) => {
        if (!ok) toast.error(t('widget.commandFailed', { name }))
      })
    },
    (name) => {
      toast.success(t('door.opened', { name }))
    },
  )
  return <WidgetView spec={spec} />
}

/**
 * Sample home rendered from local mock state, so the UI (and the screen
 * editor) can be tried without a running home. Opened with `?demo=1`.
 * Edits stay in this browser tab.
 */
export function DemoApp() {
  const { t } = useTranslation()
  const dashboard = useDemoDashboardStore((s) => s.dashboard)
  const revision = useDemoDashboardStore((s) => s.revision)
  const items = useMemo(() => demoItems(t), [t])

  // The sample charger runs while the demo is on screen.
  useEffect(() => startDemoCharger(), [])

  useEffect(() => {
    // Built once, in the language the screen starts in (like a real home).
    if (useDemoDashboardStore.getState().dashboard === null) {
      useDemoDashboardStore.setState({ dashboard: demoDashboard(t) })
    }
  }, [t])

  return (
    <DashboardScreen
      dashboard={dashboard}
      revision={revision}
      placeholder={null}
      renderWidget={(widget) => <DemoWidget widget={widget} />}
      items={items}
      itemsReady
      save={saveDemoDashboard}
      headerExtra={
        <Badge variant="secondary" className="h-auto gap-1.5 px-3 py-1.5 text-sm" title={t('demo.notice')}>
          <Icon name="demo" />
          {t('demo.badge')}
        </Badge>
      }
    />
  )
}
