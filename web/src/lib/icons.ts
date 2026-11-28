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
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Bath,
  BedDouble,
  BatteryCharging,
  BatteryFull,
  BatteryLow,
  BatteryMedium,
  BellRing,
  Blinds,
  CalendarClock,
  CarFront,
  Cctv,
  Check,
  ChevronsDown,
  ChevronsUp,
  CircleCheck,
  CircleQuestionMark,
  Clock,
  CloudSun,
  CookingPot,
  DoorClosed,
  DoorOpen,
  EvCharger,
  Eye,
  Fan,
  Fence,
  FlaskConical,
  Gauge,
  Hourglass,
  House,
  HousePlug,
  HouseWifi,
  Info,
  Languages,
  LayoutList,
  Lightbulb,
  LoaderCircle,
  Lock,
  LockOpen,
  Moon,
  OctagonX,
  Palette,
  Pause,
  PencilLine,
  Play,
  Plug,
  PlugZap,
  Plus,
  RefreshCw,
  RobotVacuum,
  RotateCcw,
  RotateCw,
  Maximize2,
  Settings,
  SlidersHorizontal,
  Snowflake,
  Sofa,
  Square,
  SquarePen,
  Sun,
  SunMoon,
  ToggleRight,
  Trash,
  Trees,
  TriangleExclamationPoint,
  Undo2,
  UtilityPole,
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
  robot_vacuum: RobotVacuum,
  // Batteries (robot vacuums)
  battery: BatteryMedium,
  battery_full: BatteryFull,
  battery_low: BatteryLow,
  // Energy flow (EV charger)
  grid: UtilityPole,
  solar: Sun,
  car: CarFront,
  home_battery: BatteryMedium,
  charging: BatteryCharging,
  plugged_in: PlugZap,
  weather: CloudSun,
  clock: Clock,

  // App chrome
  settings: Settings,
  display: Palette,
  language: Languages,
  scheme_system: SunMoon,
  scheme_light: Sun,
  scheme_dark: Moon,
  demo: FlaskConical,
  edit: SquarePen,
  pages: LayoutList,

  // States
  loading: LoaderCircle,
  /** Busy, when the screen asks for less motion (a spinner that does not spin). */
  busy: Hourglass,
  success: CircleCheck,
  info: Info,
  warning: TriangleExclamationPoint,
  error: OctagonX,
  unavailable: WifiOff,
  watch_only: Eye,
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
  add: Plus,
  remove: Trash,
  rename: PencilLine,
  resize: Maximize2,
  move_earlier: ArrowLeft,
  move_later: ArrowRight,
  move_up: ArrowUp,
  move_down: ArrowDown,
  undo: Undo2,
  pause: Pause,
  resume: Play,
  to_dock: HousePlug,
  unlock: LockOpen,
  schedule: CalendarClock,
  more_controls: SlidersHorizontal,

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
