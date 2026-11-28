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
  commandDemoRobot,
  DEMO_VOLTS,
  plugCurrentA,
  startDemoAppliances,
  switchDemoPlug,
  useDemoAppliances,
  type DemoAppliances,
} from '@/demo/demo-appliances'
import {
  changeDemoCharger,
  DEMO_CHARGER_MODES,
  DEMO_CURRENT_RANGE,
  DEMO_SCHEDULE_FEATURES,
  saveDemoSchedule,
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
  appliances: DemoAppliances,
  rows: number,
  run: (name: string, command: Promise<boolean>) => Promise<boolean>,
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
          onToggle: (on) => run(label, demo.setLight(id, { on })),
          // Dimming a light that is off switches it on.
          onBrightnessChange: (brightness) => run(label, demo.setLight(id, { brightness, on: true })),
          onColorTemperatureChange: (kelvin) => {
            const current = state.colorTemperature
            if (current === undefined) return undefined
            return run(label, demo.setLight(id, { colorTemperature: { ...current, kelvin }, on: true }))
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
          onToggle: (on) => run(label, demo.setFan(id, { on })),
          onSpeedChange: (speed) => run(label, demo.setFan(id, { speed, on: true })),
          onDirectionChange: (direction) => run(label, demo.setFan(id, { direction })),
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
          onOpen: () =>
            demo.openDoor(id).then((ok) => {
              if (ok) onDoorOpened(label)
              return ok
            }),
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
          chargingSchedule: charger.chargingSchedule,
          scheduleFeatures: DEMO_SCHEDULE_FEATURES,
          readings: charger.readings,
          onPause: () => run(label, changeDemoCharger({ paused: true })),
          onResume: () => run(label, changeDemoCharger({ paused: false })),
          onCurrentChange: (amps) => run(label, changeDemoCharger({ currentA: amps })),
          onModeChange: (mode) => run(label, changeDemoCharger({ mode })),
          onLockChange: (locked) => run(label, changeDemoCharger({ locked })),
          onScheduleChange: (enabled) => run(label, changeDemoCharger({ scheduleEnabled: enabled })),
          onChargingScheduleChange: (schedule) => run(label, saveDemoSchedule(schedule)),
        },
      }
    case 'plug': {
      const plug = appliances.plugs[thing.id]
      return {
        type: 'outlet-control',
        props: {
          label,
          on: plug.on,
          pending: plug.pending,
          powerW: plug.powerW,
          energyWh: plug.energyWh,
          voltageV: DEMO_VOLTS,
          currentA: plugCurrentA(plug),
          onToggle: plug.watchOnly
            ? undefined
            : (on) => run(label, switchDemoPlug(thing.id, on)),
        },
      }
    }
    case 'robot': {
      const { robot } = appliances
      return {
        type: 'robot-vacuum',
        props: {
          label,
          pending: robot.pending,
          status: robot.status,
          batteryPercent: robot.batteryPercent,
          batteryLow: robot.batteryPercent <= 20,
          cleanedAreaM2: robot.cleanedAreaM2,
          cleaningDurationS: robot.cleaningDurationS,
          onStart: () => run(label, commandDemoRobot('start')),
          onPause: () => run(label, commandDemoRobot('pause')),
          onReturnToDock: () => run(label, commandDemoRobot('dock')),
        },
      }
    }
  }
}

/** One tile of the sample home. */
function DemoWidget({ widget }: { widget: Widget }) {
  const { t } = useTranslation()
  const demo = useDemoStore()
  const charger = useDemoCharger()
  const appliances = useDemoAppliances()

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
    appliances,
    widget.row_span,
    (name, command) =>
      command.then((ok) => {
        if (!ok) toast.error(t('widget.commandFailed', { name }))
        return ok
      }),
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

  // The sample charger, plugs and robot run while the demo is on screen.
  useEffect(() => startDemoCharger(), [])
  useEffect(() => startDemoAppliances(), [])

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
