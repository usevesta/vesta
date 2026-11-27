/**
 * Semantic icon registry.
 *
 * Components never import icons directly: they ask for a semantic name
 * (`<Icon name="fan" />`) and this file decides how it looks. Swapping the
 * icon set later is a change to this file only, like swapping a theme.
 *
 * Names are lowercase `snake_case` so they match the values stored in
 * `dashboard.json` (page icons) and endpoint types (`garage_door`, ...).
 * Only Lucide (ISC license, redistributable) is allowed here.
 */
import {
  Activity,
  Bath,
  BedDouble,
  BellRing,
  Blinds,
  Cctv,
  Check,
  ChevronsDown,
  ChevronsUp,
  CircleCheck,
  CircleQuestionMark,
  CloudSun,
  CookingPot,
  DoorClosed,
  DoorOpen,
  EvCharger,
  Fan,
  Fence,
  FlaskConical,
  Gauge,
  House,
  HouseWifi,
  Info,
  Languages,
  Lightbulb,
  LoaderCircle,
  Lock,
  Moon,
  OctagonX,
  Palette,
  Plug,
  RefreshCw,
  RotateCcw,
  RotateCw,
  Settings,
  Snowflake,
  Sofa,
  Square,
  Sun,
  SunMoon,
  ToggleRight,
  Trees,
  TriangleExclamationPoint,
  VideoOff,
  Warehouse,
  WifiOff,
  X,
  Zap,
  type LucideIcon,
} from 'lucide-react'

export type IconComponent = LucideIcon

export const ICONS = {
  // Places (page icons)
  home: House,
  living_room: Sofa,
  bedroom: BedDouble,
  kitchen: CookingPot,
  bathroom: Bath,
  garden: Trees,

  // Things in the home (endpoint types and friends)
  light: Lightbulb,
  outlet: Plug,
  switch: ToggleRight,
  fan: Fan,
  blind: Blinds,
  window_covering: Blinds,
  garage: Warehouse,
  garage_door: Warehouse,
  gate: Fence,
  door: DoorClosed,
  door_open: DoorOpen,
  lock: Lock,
  camera: Cctv,
  doorbell: BellRing,
  sensor: Activity,
  power: Zap,
  meter: Gauge,
  ev_charger: EvCharger,
  weather: CloudSun,

  // App chrome
  settings: Settings,
  display: Palette,
  language: Languages,
  scheme_system: SunMoon,
  scheme_light: Sun,
  scheme_dark: Moon,
  demo: FlaskConical,

  // States
  loading: LoaderCircle,
  success: CircleCheck,
  info: Info,
  warning: TriangleExclamationPoint,
  error: OctagonX,
  unavailable: WifiOff,
  connection_lost: HouseWifi,
  video_off: VideoOff,

  // Actions
  close: X,
  check: Check,
  retry: RefreshCw,
  cover_open: ChevronsUp,
  cover_close: ChevronsDown,
  cover_stop: Square,
  direction_forward: RotateCw,
  direction_reverse: RotateCcw,
  warm: Sun,
  cool: Snowflake,

  // Fallback for names this version does not know
  unknown: CircleQuestionMark,
} as const satisfies Record<string, IconComponent>

export type IconName = keyof typeof ICONS

export function isIconName(name: string): name is IconName {
  return Object.hasOwn(ICONS, name)
}

/** Resolve any string (e.g. a page icon from `dashboard.json`), never failing. */
export function resolveIcon(name: string | null | undefined): IconComponent {
  return name != null && isIconName(name) ? ICONS[name] : ICONS.unknown
}
