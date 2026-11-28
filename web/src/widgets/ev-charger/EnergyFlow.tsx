import type { CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import type { IconName } from '@/lib/icons'
import { cn } from '@/lib/utils'
import {
  flowDuration,
  flowLinks,
  formatKw,
  headroom,
  houseConsumption,
  type EnergyReadings,
  type FlowLink,
  type FlowNode,
} from '@/widgets/ev-charger/energy'

/*
 * Geometry, in diagram units. The house sits in the middle; the grid on the
 * left, the car on the right, the sun above and a home battery below, each
 * joined to the house by a straight conduit. Straight lines keep the bead
 * animation cheap and the picture calm.
 */
const WIDTH = 360
const NODE_R = 24
const HOUSE_R = 31
const GAUGE_R = HOUSE_R + 5
const GAUGE_LENGTH = 2 * Math.PI * GAUGE_R
const X = { grid: 50, house: 180, car: 310 } as const
const GAP = 5

interface NodeStyle {
  readonly icon: IconName
  /** Tailwind classes, spelled out so the scanner keeps them. */
  readonly text: string
  readonly fill: string
  readonly ring: string
  readonly stroke: string
}

const STYLE: Readonly<Record<FlowNode | 'house', NodeStyle>> = {
  grid: {
    icon: 'grid',
    text: 'text-energy-grid',
    fill: 'fill-energy-grid/12',
    ring: 'stroke-energy-grid/45',
    stroke: 'stroke-energy-grid',
  },
  solar: {
    icon: 'solar',
    text: 'text-energy-solar',
    fill: 'fill-energy-solar/14',
    ring: 'stroke-energy-solar/50',
    stroke: 'stroke-energy-solar',
  },
  battery: {
    icon: 'home_battery',
    text: 'text-energy-battery',
    fill: 'fill-energy-battery/12',
    ring: 'stroke-energy-battery/45',
    stroke: 'stroke-energy-battery',
  },
  car: {
    icon: 'car',
    text: 'text-energy-car',
    fill: 'fill-energy-car/12',
    ring: 'stroke-energy-car/50',
    stroke: 'stroke-energy-car',
  },
  house: {
    icon: 'home',
    text: 'text-energy-house',
    fill: 'fill-energy-house/8',
    ring: 'stroke-energy-house/30',
    stroke: 'stroke-energy-house',
  },
}

interface Point {
  readonly x: number
  readonly y: number
}

interface Layout {
  readonly height: number
  readonly house: Point
  readonly nodes: Readonly<Record<FlowNode, Point>>
}

function layout(hasSolar: boolean, hasBattery: boolean): Layout {
  const top = 30
  const middle = hasSolar ? 142 : 44
  const bottom = middle + 110
  return {
    height: hasBattery ? bottom + NODE_R + 8 : middle + NODE_R + 44,
    house: { x: X.house, y: middle },
    nodes: {
      grid: { x: X.grid, y: middle },
      car: { x: X.car, y: middle },
      solar: { x: X.house, y: top },
      battery: { x: X.house, y: bottom },
    },
  }
}

/** The conduit from `node` to the house, outer end first. */
function conduit(node: FlowNode, at: Point, house: Point): [Point, Point] {
  switch (node) {
    case 'grid':
      return [
        { x: at.x + NODE_R + GAP, y: at.y },
        { x: house.x - GAUGE_R - GAP, y: house.y },
      ]
    case 'car':
      return [
        { x: at.x - NODE_R - GAP, y: at.y },
        { x: house.x + GAUGE_R + GAP, y: house.y },
      ]
    case 'solar':
      return [
        { x: at.x, y: at.y + NODE_R + GAP },
        { x: house.x, y: house.y - GAUGE_R - GAP },
      ]
    case 'battery':
      return [
        { x: at.x, y: at.y - NODE_R - GAP },
        { x: house.x, y: house.y + GAUGE_R + GAP },
      ]
  }
}

function Conduit({ link, from, to }: { link: FlowLink; from: Point; to: Point }) {
  const style = STYLE[link.node]
  const d = `M ${String(from.x)} ${String(from.y)} L ${String(to.x)} ${String(to.y)}`
  const beads: CSSProperties = { '--flow-duration': flowDuration(link.speed) } as CSSProperties
  // Where the power goes: towards the house (`in`) or away from it.
  const [source, target] = link.direction === 'in' ? [from, to] : [to, from]
  const angle = (Math.atan2(target.y - source.y, target.x - source.x) * 180) / Math.PI
  const mid = { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 }
  const reverse = link.direction === 'out' ? true : undefined
  return (
    <g data-node={link.node} data-active={link.active || undefined}>
      <path d={d} className={cn(style.stroke, 'opacity-20')} strokeWidth={2} strokeLinecap="round" fill="none" />
      {link.active && (
        <>
          <path
            d={d}
            className={cn('energy-beads opacity-25', style.stroke)}
            style={beads}
            data-reverse={reverse}
            strokeWidth={link.width + 5}
            fill="none"
          />
          <path
            d={d}
            className={cn('energy-beads', style.stroke)}
            style={beads}
            data-reverse={reverse}
            strokeWidth={link.width}
            fill="none"
          />
          <path
            className={cn('energy-arrow', style.stroke)}
            d="M -4 -5 L 2 0 L -4 5"
            transform={`translate(${String(mid.x)} ${String(mid.y)}) rotate(${String(angle)})`}
            strokeWidth={2.5}
            strokeLinecap="round"
            strokeLinejoin="round"
            fill="none"
          />
        </>
      )}
    </g>
  )
}

interface NodeProps {
  node: FlowNode
  at: Point
  active: boolean
  value: string
  name: string
  /** Where the labels go: under the node, or to its right. */
  labels: 'below' | 'right'
}

function Node({ node, at, active, value, name, labels }: NodeProps) {
  const style = STYLE[node]
  const iconSize = 22
  const icon = (
    <Icon
      name={style.icon}
      x={at.x - iconSize / 2}
      y={at.y - iconSize / 2}
      size={iconSize}
      strokeWidth={1.8}
      className={active ? style.text : 'text-muted-foreground'}
    />
  )
  const label =
    labels === 'below'
      ? { x: at.x, y: at.y + NODE_R + 20, anchor: 'middle' as const, gap: 15 }
      : { x: at.x + NODE_R + 12, y: at.y - 1, anchor: 'start' as const, gap: 15 }
  return (
    <g data-node={node}>
      {node === 'car' && active && (
        <>
          <circle cx={at.x} cy={at.y} r={NODE_R} className={cn('energy-halo', style.stroke)} strokeWidth={2} fill="none" />
          <circle
            cx={at.x}
            cy={at.y}
            r={NODE_R}
            className={cn('energy-halo', style.stroke)}
            style={{ animationDelay: '-1.3s' }}
            strokeWidth={2}
            fill="none"
          />
        </>
      )}
      <circle
        cx={at.x}
        cy={at.y}
        r={NODE_R}
        className={cn('transition-colors duration-500', active ? [style.fill, style.ring] : 'fill-muted stroke-border')}
        strokeWidth={1.5}
      />
      {node === 'solar' && active ? <g className="energy-spin">{icon}</g> : icon}
      <text
        x={label.x}
        y={label.y}
        textAnchor={label.anchor}
        className="fill-foreground font-semibold tabular-nums"
        fontSize={15}
      >
        {value}
      </text>
      <text
        x={label.x}
        y={label.y + label.gap}
        textAnchor={label.anchor}
        className="fill-muted-foreground"
        fontSize={11}
      >
        {name}
      </text>
    </g>
  )
}

export interface EnergyFlowProps {
  readings: EnergyReadings
  /** The tile is not responding: show the last readings, frozen. */
  frozen?: boolean
  className?: string
}

/**
 * Where the home's power comes from and where it goes, as a small living
 * diagram: beads flow along each line (faster and thicker for more
 * power), the car breathes while it charges, and a ring around the house
 * fills towards the contracted power.
 */
export function EnergyFlow({ readings, frozen = false, className }: EnergyFlowProps) {
  const { t, i18n } = useTranslation()
  const links = flowLinks(readings)
  const byNode = new Map(links.map((link) => [link.node, link]))
  const geometry = layout(byNode.has('solar'), byNode.has('battery'))
  const { house } = geometry
  const kw = (watts: number) => t('ev.kw', { value: formatKw(watts, i18n.language) })
  const houseW = houseConsumption(readings)
  const room = headroom(readings)
  const near = room?.nearLimit === true

  const names: Record<FlowNode, string> = {
    grid: t('ev.nodes.grid'),
    solar: t('ev.nodes.solar'),
    battery: t('ev.nodes.battery'),
    car: t('ev.nodes.car'),
  }
  const summary = [
    ...links.map((link) => `${names[link.node]} ${kw(link.watts)}`),
    ...(houseW === undefined ? [] : [`${t('ev.nodes.house')} ${kw(houseW)}`]),
  ].join(', ')

  return (
    <svg
      viewBox={`0 0 ${String(WIDTH)} ${String(geometry.height)}`}
      role="img"
      aria-label={t('ev.flowLabel', { summary })}
      data-frozen={frozen || undefined}
      className={cn('energy-layer block h-auto w-full overflow-visible select-none', className)}
    >
      {links.map((link) => {
        const [from, to] = conduit(link.node, geometry.nodes[link.node], house)
        return <Conduit key={link.node} link={link} from={from} to={to} />
      })}

      {/* The house: consumption inside, the contracted power as a ring. */}
      <g data-node="house">
        <circle cx={house.x} cy={house.y} r={GAUGE_R} className="stroke-muted" strokeWidth={4} fill="none" />
        {room !== null && (
          <circle
            cx={house.x}
            cy={house.y}
            r={GAUGE_R}
            className={cn('energy-gauge', near ? 'stroke-energy-limit' : 'stroke-energy-house/70')}
            strokeWidth={4}
            strokeLinecap="round"
            strokeDasharray={`${String(room.ratio * GAUGE_LENGTH)} ${String(GAUGE_LENGTH)}`}
            transform={`rotate(-90 ${String(house.x)} ${String(house.y)})`}
            fill="none"
          />
        )}
        <circle
          cx={house.x}
          cy={house.y}
          r={HOUSE_R}
          className={cn(STYLE.house.fill, STYLE.house.ring)}
          strokeWidth={1.5}
        />
        <Icon
          name="home"
          x={house.x - 10}
          y={house.y - 19}
          size={20}
          strokeWidth={1.8}
          className={near ? 'text-energy-limit' : STYLE.house.text}
        />
        <text
          x={house.x}
          y={house.y + 15}
          textAnchor="middle"
          className="fill-foreground font-semibold tabular-nums"
          fontSize={12.5}
        >
          {houseW === undefined ? '–' : kw(houseW)}
        </text>
      </g>

      {links.map((link) => (
        <Node
          key={link.node}
          node={link.node}
          at={geometry.nodes[link.node]}
          active={link.active}
          value={kw(link.watts)}
          name={
            link.active && link.direction === 'out' && link.node === 'grid'
              ? t('ev.nodes.gridExport')
              : link.active && link.direction === 'out' && link.node === 'battery'
                ? t('ev.nodes.batteryCharging')
                : names[link.node]
          }
          labels={link.node === 'solar' || link.node === 'battery' ? 'right' : 'below'}
        />
      ))}
    </svg>
  )
}
