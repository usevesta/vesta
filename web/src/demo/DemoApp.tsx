import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { DashboardGrid, DashboardGridItem } from '@/components/dashboard/DashboardGrid'
import { Icon } from '@/components/Icon'
import { AppShell } from '@/components/shell/AppShell'
import type { ShellPage } from '@/components/shell/PageNav'
import { Badge } from '@/components/ui/badge'
import {
  useDemoStore,
  type DemoCoverId,
  type DemoLightId,
  type DemoState,
} from '@/demo/demo-store'
import type { IconName } from '@/lib/icons'
import type { AnyWidgetSpec, WidgetSpec } from '@/widgets/registry'
import { WidgetView } from '@/widgets/WidgetView'

type DemoPageId = 'home' | 'livingRoom' | 'outside'

interface DemoTile {
  key: string
  colSpan: number
  rowSpan: number
  spec: WidgetSpec | AnyWidgetSpec
}

interface DemoPage {
  columns: number
  tiles: DemoTile[]
}

/**
 * Sample home rendered from local mock state, so the UI can be reviewed
 * without a running home. Opened with `?demo=1`.
 */
export function DemoApp() {
  const { t } = useTranslation()
  const [pageId, setPageId] = useState<DemoPageId>('home')
  const demo: DemoState = useDemoStore()

  const run = (name: string, command: Promise<boolean>) => {
    void command.then((ok) => {
      if (!ok) toast.error(t('widget.commandFailed', { name }))
    })
  }

  const light = (id: DemoLightId, label: string): WidgetSpec<'light-control'> => {
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

  const cover = (
    id: DemoCoverId,
    label: string,
    icon: IconName,
    sensitive = false,
  ): WidgetSpec<'cover-control'> => {
    const state = demo.covers[id]
    return {
      type: 'cover-control',
      props: {
        label,
        icon,
        sensitive,
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

  const fan = demo.fans.ceiling
  const ceilingFanLabel = t('demo.devices.ceilingFan')
  const ceilingFan: WidgetSpec<'fan-control'> = {
    type: 'fan-control',
    props: {
      label: ceilingFanLabel,
      on: fan.on,
      speed: fan.speed,
      speedSteps: fan.speedSteps,
      direction: fan.direction,
      pending: fan.pending,
      onToggle: (on) => {
        run(ceilingFanLabel, demo.setFan('ceiling', { on }))
      },
      onSpeedChange: (speed) => {
        run(ceilingFanLabel, demo.setFan('ceiling', { speed, on: true }))
      },
      onDirectionChange: (direction) => {
        run(ceilingFanLabel, demo.setFan('ceiling', { direction }))
      },
    },
  }

  const frontDoorLabel = t('demo.devices.frontDoor')
  const frontDoor: WidgetSpec<'door-button'> = {
    type: 'door-button',
    props: {
      label: frontDoorLabel,
      pending: demo.doors.front.pending,
      onOpen: () => {
        void demo.openDoor('front').then((ok) => {
          if (ok) toast.success(t('door.opened', { name: frontDoorLabel }))
        })
      },
    },
  }

  const clock: WidgetSpec<'clock-weather'> = {
    type: 'clock-weather',
    props: { label: t('demo.devices.clockPlace') },
  }

  const camera = (label: string, unavailable = false): WidgetSpec<'camera-preview'> => ({
    type: 'camera-preview',
    props: { label, unavailable },
  })

  const pages: Record<DemoPageId, DemoPage> = {
    home: {
      columns: 4,
      tiles: [
        { key: 'clock', colSpan: 2, rowSpan: 1, spec: clock },
        { key: 'front-door', colSpan: 2, rowSpan: 1, spec: frontDoor },
        { key: 'front-camera', colSpan: 2, rowSpan: 2, spec: camera(t('demo.devices.frontDoorCamera')) },
        { key: 'living-light', colSpan: 2, rowSpan: 1, spec: light('livingRoom', t('demo.devices.livingRoomLight')) },
        { key: 'garage', colSpan: 2, rowSpan: 1, spec: cover('garage', t('demo.devices.garageDoor'), 'garage_door', true) },
      ],
    },
    livingRoom: {
      columns: 4,
      tiles: [
        { key: 'living-light', colSpan: 2, rowSpan: 1, spec: light('livingRoom', t('demo.devices.livingRoomLight')) },
        { key: 'ceiling-fan', colSpan: 2, rowSpan: 1, spec: ceilingFan },
        { key: 'fan-light', colSpan: 2, rowSpan: 1, spec: light('fanLight', t('demo.devices.fanLight')) },
        { key: 'blind', colSpan: 2, rowSpan: 1, spec: cover('bedroomBlind', t('demo.devices.bedroomBlind'), 'blind') },
        { key: 'kitchen', colSpan: 1, rowSpan: 1, spec: light('kitchen', t('demo.devices.kitchenLight')) },
        { key: 'hallway', colSpan: 1, rowSpan: 1, spec: light('hallway', t('demo.devices.hallwayLight')) },
      ],
    },
    outside: {
      columns: 4,
      tiles: [
        { key: 'garage', colSpan: 2, rowSpan: 1, spec: cover('garage', t('demo.devices.garageDoor'), 'garage_door', true) },
        { key: 'awning', colSpan: 2, rowSpan: 1, spec: cover('patioAwning', t('demo.devices.patioAwning'), 'window_covering') },
        { key: 'porch', colSpan: 2, rowSpan: 1, spec: light('porch', t('demo.devices.porchLight')) },
        { key: 'garden-camera', colSpan: 2, rowSpan: 2, spec: camera(t('demo.devices.gardenCamera'), true) },
        // A widget type this version does not know: shows the placeholder.
        { key: 'future', colSpan: 2, rowSpan: 1, spec: { type: 'thermostat-control', props: {} } },
      ],
    },
  }

  const shellPages: ShellPage[] = [
    { id: 'home', title: t('demo.pages.home'), icon: 'home' },
    { id: 'livingRoom', title: t('demo.pages.livingRoom'), icon: 'living_room' },
    { id: 'outside', title: t('demo.pages.outside'), icon: 'garden' },
  ]

  const page = pages[pageId]

  return (
    <AppShell
      pages={shellPages}
      activePageId={pageId}
      onSelectPage={(id) => {
        if (id === 'home' || id === 'livingRoom' || id === 'outside') setPageId(id)
      }}
      headerExtra={
        <Badge variant="secondary" className="h-auto gap-1.5 px-3 py-1.5 text-sm" title={t('demo.notice')}>
          <Icon name="demo" />
          {t('demo.badge')}
        </Badge>
      }
    >
      <DashboardGrid columns={page.columns}>
        {page.tiles.map((tile) => (
          <DashboardGridItem key={tile.key} colSpan={tile.colSpan} rowSpan={tile.rowSpan}>
            <WidgetView spec={tile.spec} />
          </DashboardGridItem>
        ))}
      </DashboardGrid>
    </AppShell>
  )
}
