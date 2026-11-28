# Capability Model

Status: **accepted (v1 draft)**. This document defines how VESTA describes hardware to the Hub and the frontend without ever naming a brand.

## Principles
1. **Brand-free.** The frontend and `vesta-hub` only see endpoints and capabilities. Brands live exclusively inside `crates/drivers/*`.
2. **Matter-inspired.** The structure mirrors Matter's data model (node → endpoint → cluster) so a future Matter bridge is a translation table, not a rewrite.
3. **Reported by drivers.** A driver describes what the hardware can do. Users can change *presentation* (name, room, endpoint type, meter role), never capabilities.
4. **Data, not trait objects.** Capabilities, commands, state and events are plain serializable Rust types (`serde`), exported to TypeScript. One `DeviceDriver` trait executes typed commands.
5. **Honest features.** Each capability declares static `features` (what the hardware can do) separately from dynamic `state`. The UI renders controls from `features`; drivers never fake a feature.

## Structure

```
Device                      a physical unit known to one driver (e.g. a ceiling fan)
└── Endpoint                one controllable function of it (e.g. the fan, the light)
    ├── type                presentation: icon, default widget, sensitivity
    └── Capability          on_off, level, cover, …
        ├── features        static, reported by the driver
        └── state           dynamic, updated by the driver
```

Why endpoints: a ceiling fan with a light has **two** things that switch on and off. A flat device model makes "turn on" ambiguous. The same applies to multi-channel relays, power strips, or an EV charger that is also a household power meter.

### Addressing
- Device IDs and endpoint IDs are stable, lowercase `snake_case`.
- An endpoint is addressed as **`<device_id>/<endpoint_id>`** (a *target*), e.g. `living_room_fan/light`.
- Single-function devices expose one endpoint named `main`.

### Example (wire format)

```json
{
  "id": "living_room_fan",
  "name": "Living Room Fan",
  "room": "living_room",
  "online": true,
  "endpoints": [
    {
      "id": "fan",
      "name": "Fan",
      "type": "fan",
      "capabilities": {
        "on_off": { "state": { "on": true } },
        "fan": {
          "features": { "speed_steps": 6, "reversible": true },
          "state": { "speed": 3, "direction": "forward" }
        }
      }
    },
    {
      "id": "light",
      "name": "Light",
      "type": "light",
      "capabilities": {
        "on_off": { "state": { "on": false } },
        "level": { "state": { "level": 80 } },
        "color_temperature": {
          "features": { "min_kelvin": 2700, "max_kelvin": 6500 },
          "state": { "kelvin": 3000 }
        }
      }
    }
  ]
}
```

- `capabilities` is an object keyed by capability name. `features` is omitted for capabilities that have none; `state` is omitted while the driver does not know it yet (it never invents a value). A capability with neither (`trigger`, `doorbell`) is an empty object: `"trigger": {}`.
- Optional fields: device `last_seen` (RFC 3339) and endpoint `room`, which overrides the device's room.

### Example: a car charger and its meters

A charger that also reads the home's grid and consumption is one device with an endpoint per
function. The grid meter carries the contracted power as `limit_w`.

```json
{
  "id": "car_charger",
  "name": "Car Charger",
  "room": "garage",
  "online": true,
  "endpoints": [
    {
      "id": "charger",
      "name": "Car Charger",
      "type": "ev_charger",
      "capabilities": {
        "ev_charger": {
          "features": {
            "min_current_a": 6.0,
            "max_current_a": 32.0,
            "modes": [],
            "pause": true,
            "adjustable_current": true,
            "lock": true,
            "schedule": true,
            "charging_schedule": {
              "max_windows": 2,
              "weekdays": true,
              "overnight": true,
              "editable": true,
              "reports_windows": false,
              "saving_needs_internet": true,
              "refreshable": true
            }
          },
          "state": {
            "status": "charging",
            "power_w": 5092.0,
            "session_energy_wh": 4890.0,
            "session_duration_s": 3841,
            "current_a": 23.0,
            "auto_current": true,
            "phase_current_a": [22.94, 0.0, 0.0],
            "min_current_a": 6.0,
            "max_current_a": 30.0,
            "mode": "scheduled",
            "locked": false,
            "schedule_enabled": true,
            "charging_schedule": {
              "enabled": true,
              "windows": [
                { "start": "22:00", "end": "06:00", "days": ["monday", "tuesday", "wednesday", "thursday", "friday"] },
                { "start": "00:00", "end": "08:00", "days": ["saturday", "sunday"] }
              ],
              "origin": { "source": "charger", "at": "2026-11-28T09:30:00Z", "restored": false }
            }
          }
        }
      }
    },
    {
      "id": "grid",
      "name": "Grid",
      "type": "meter",
      "capabilities": {
        "power_meter": {
          "features": { "role": "grid", "bidirectional": true },
          "state": { "power_w": 5739.0, "limit_w": 5700.0 }
        }
      }
    },
    {
      "id": "house",
      "name": "House",
      "type": "meter",
      "capabilities": {
        "power_meter": {
          "features": { "role": "house", "bidirectional": false },
          "state": { "power_w": 5739.0 }
        }
      }
    }
  ]
}
```

### Example: a plug that is only watched, and a robot vacuum

A plug set up for monitoring only (`read_only` in its driver config) says so in its `on_off`
features: the UI shows whether it is on, without a switch. Its meter is a `load`. A robot vacuum
reports its charge as a `battery` capability on the same endpoint.

```json
{
  "id": "hall_plug",
  "name": "Hall lamp",
  "room": "hall",
  "online": true,
  "endpoints": [
    {
      "id": "main",
      "name": "Hall lamp",
      "type": "outlet",
      "capabilities": {
        "on_off": { "features": { "read_only": true }, "state": { "on": true } },
        "power_meter": {
          "features": { "role": "load", "bidirectional": false },
          "state": { "power_w": 12.5, "voltage_v": 227.2, "current_a": 0.112 }
        }
      }
    }
  ]
}
```

```json
{
  "id": "robot_vacuum",
  "name": "Robot vacuum",
  "online": true,
  "endpoints": [
    {
      "id": "main",
      "name": "Robot vacuum",
      "type": "robot_vacuum",
      "capabilities": {
        "battery": { "state": { "percent": 100, "low": false } },
        "robot_vacuum": {
          "features": { "start": true, "pause": true, "return_to_dock": true },
          "state": { "status": "docked", "cleaned_area_m2": 22.0, "cleaning_duration_s": 1800 }
        }
      }
    }
  ]
}
```

## Endpoint Types
The type decides the icon, the default widget and whether opening commands are sensitive. Capabilities decide the controls. Drivers report a default type; users may override it from the UI (e.g. "this relay is a light").

| Type | Typical capabilities | Sensitive opening/unlock |
|---|---|---|
| `light` | `on_off`, `level`, `color_temperature`, `color` | no |
| `outlet` | `on_off`, `power_meter` | no |
| `switch` | `on_off` | no |
| `fan` | `on_off`, `fan` | no |
| `window_covering` | `cover` (blinds, shutters, curtains, awnings) | no |
| `garage_door` | `cover` | **yes** |
| `gate` | `cover` or `trigger` | **yes** |
| `door` | `trigger` (electric strike, door opener) | **yes** |
| `lock` | `lock` | **yes** |
| `camera` | `camera`, `occupancy` | no |
| `doorbell` | `doorbell`, `camera`, `occupancy` | no |
| `sensor` | `occupancy`, `contact`, `temperature`, `humidity`, `battery` | no |
| `meter` | `power_meter` | no |
| `ev_charger` | `ev_charger` (its meters are sibling `meter` endpoints, see below) | no (but `ev_charger.unlock` is sensitive) |
| `robot_vacuum` | `robot_vacuum`, `battery` | no |

## Capability Catalog

| Capability | Features | State | Commands | Events | Phase |
|---|---|---|---|---|---|
| `on_off` | `read_only: bool` (optional; omitted = switchable) | `on: bool` | `on`, `off`, `toggle` | — | v1 |
| `level` | — | `level: 0–100` (%) | `set { level, transition_ms? }` | — | v1 |
| `color_temperature` | `min_kelvin`, `max_kelvin` | `kelvin` | `set { kelvin, transition_ms? }` | — | v1 |
| `color` | — | `hue: 0–360`, `saturation: 0–100` | `set { hue, saturation, transition_ms? }` | — | v1 |
| `cover` | `kind` (`blind`, `shutter`, `curtain`, `awning`, `garage`, `gate`), `position: bool`, `stop: bool` | `position: 0–100?`, `motion: opening \| closing \| stopped`, `open: bool?` | `open`, `close`, `stop`, `set_position { position }` | — | v1 |
| `fan` | `speed_steps: N`, `reversible: bool` | `speed: 1–N`, `direction: forward \| reverse` | `set_speed { speed }`, `set_direction { direction }` | — | v1 |
| `lock` | `unlatch: bool` | `lock_state: locked \| unlocked \| jammed \| unknown` | `lock`, `unlock`, `unlatch` | — | v2 |
| `trigger` | — | — | `fire` | — | v2 |
| `camera` | `two_way_audio: bool`, `snapshot: bool` | `stream_available: bool` | — (streams start through the signaling API) | — | v2 |
| `doorbell` | — | — | — | `doorbell.pressed` | v2 |
| `occupancy` | — | `occupied: bool` | — | `occupancy.detected` | v2 |
| `contact` | — | `open: bool` | — | — | v2 |
| `temperature` | — | `celsius` | — | — | v2 |
| `humidity` | — | `percent: 0–100` | — | — | v2 |
| `power_meter` | `role` (`grid`, `solar`, `load`, `ev`, `house`, `battery`), `bidirectional: bool` | `power_w`, `energy_wh?`, `energy_exported_wh?`, `limit_w?`, `voltage_v?`, `current_a?` | — | — | v2 |
| `ev_charger` | `min_current_a`, `max_current_a`, `modes` (`manual`, `solar`, `solar_grid`, `scheduled`), `pause`, `adjustable_current`, `lock`, `schedule` (bools), `charging_schedule?` (`max_windows`, `weekdays`, `overnight`, `editable`, `reports_windows`, `saving_needs_internet`) | `status: disconnected \| connected \| charging \| paused \| waiting \| error`, `waiting_for?: schedule \| solar \| power`, `power_w`, `session_energy_wh`, `session_duration_s?`, `current_a`, `auto_current?`, `phase_current_a?: [A]`, `min_current_a?`, `max_current_a?`, `mode?`, `locked?`, `schedule_enabled?`, `charging_schedule?: { enabled, windows: [{ start, end, days }] }` | `pause`, `resume`, `set_current { amps }`, `set_mode { mode }`, `lock`, `unlock`, `set_schedule { enabled }`, `set_charging_schedule { enabled, windows }` | — | v2 |
| `battery` | — | `percent: 0–100`, `low: bool` | — | — | v2 |
| `robot_vacuum` | `start`, `pause`, `return_to_dock` (bools) | `status: docked \| charging \| cleaning \| paused \| returning \| idle \| error`, `cleaned_area_m2?`, `cleaning_duration_s?` | `start`, `pause`, `return_to_dock` | — | v2 |

Notes:
- `on_off` + `level`: `level.set` on an endpoint that is off turns it on (like Matter's *MoveToLevelWithOnOff*). Dragging a slider must light the lamp.
- `cover` without `position` (e.g. many garage doors) only offers `open` / `close` (and `stop` if supported). `open` reports the end-stop state when a contact sensor exists.
- `power_meter.role` describes *where* the clamp sits, which only the installer knows. Drivers may default it; users set it during setup. It is the one feature users may override.
- `ev_charger.set_current` is always clamped to the range the charger accepts right now: `state.min_current_a..state.max_current_a` (the limits configured on the charger) within `features` (the hardware). The device's own limits (e.g. contracted-power control) always prevail — VESTA never replaces them (AGENTS.md rule 7).
- Every `ev_charger` command needs its feature (`pause` for `pause`/`resume`, `adjustable_current`, `lock` for `lock`/`unlock`, `schedule`, and the mode in `modes`); the Hub answers `unsupported` otherwise. A charger set up for monitoring only reports them all `false` and no modes, and the UI shows no controls.
- `ev_charger.auto_current: true` means the charger's own power control chooses the current by itself (e.g. to stay within the contracted power): `set_current` would have no effect, so drivers refuse it and the UI shows the current as set by the charger.
- `ev_charger.status: waiting` means a car is plugged in and the charger holds back by itself; `waiting_for` says why (`schedule`: outside the charger's hours, `solar`: not enough surplus sun, `power`: the home uses too much for even the minimum current), only when the charger makes it clear. `paused` is someone pausing it; `connected` is plugged in with no reason given (e.g. the car is full).
- `ev_charger.current_a` is the setpoint (what the charger offers the car); `phase_current_a` is what the car draws, per phase (L1 first).
- `ev_charger.mode`: `manual` charges at the set current (as fast as allowed), `solar` only with surplus sun, `solar_grid` with surplus sun topped up from the grid to the minimum current, `scheduled` as planned on the charger. Chargers whose schedule is a separate on/off (in any mode) report `features.schedule` and `state.schedule_enabled` instead.
- `ev_charger` charging hours (Matter: no EnergyEVSE equivalent; its `ChargingTargetSchedules` are "charged by" targets, not windows). The schedule **lives in the charger**, which follows it by itself, so it keeps working when VESTA, the panel or the internet are down (AGENTS.md rule 7). VESTA only stores hours in it and switches it on and off.
  - `features.charging_schedule` says what the charger holds and what VESTA can do: up to `max_windows` time windows; `weekdays`: each window can be limited to some days (otherwise every window is every day); `overnight`: a window may run past midnight (it ends before it starts); `editable`: `set_charging_schedule` works; `reports_windows`: the charger reports its hours (when `false`, `state.charging_schedule` holds the hours last saved from VESTA, which may have been changed elsewhere since, and is omitted until then); `saving_needs_internet`: storing the hours goes through the internet (switching the schedule on and off with `set_schedule`, and following it, do not). Omitted when the charger has no windows or they can only be seen and changed on the charger (or its own app); `set_schedule` still switches them on and off.
  - `state.charging_schedule`: `enabled` (same as `schedule_enabled`) and `windows`, each `{ start, end, days }`. A window that ends before it starts runs past midnight, and `days` are the days it starts on (Friday 22:00–06:00 ends on Saturday morning). Days are in week order.
  - `set_charging_schedule { enabled, windows }` stores the windows and switches the schedule on or off in one go. It needs `features.charging_schedule.editable` and windows that fit (1 to `max_windows`, each with at least one day and no day twice, every day when the charger has no `weekdays`, start ≠ end, past midnight only with `overnight`); the Hub answers `unsupported` otherwise. It is a **slow** command: the Hub and the screens wait 20 s instead of 5 s.
- A charger and the meters it reads (grid, house, solar, battery) are **endpoints of one device**: a charger widget finds its meters among its sibling endpoints by `role`.
- `power_meter.limit_w` is the most power the connection may draw (the contracted power on a `grid` meter). It is informative only: VESTA never enforces it; the devices that do keep doing it.
- `power_meter.energy_wh` is omitted when the device only measures power (no invented counters).
  A counter that resets (e.g. "energy since the device last reported to its cloud") is not a
  lifetime counter and is not reported as one.
- `power_meter.voltage_v` and `current_a` are what the meter measures right now, when it does.
- `on_off.features.read_only: true` means VESTA only watches the switch (the device is set up for
  monitoring only): the Hub answers `unsupported` to `on`, `off` and `toggle`, and the UI shows the
  state without a switch. Without `features` (the usual case) it can be switched.
- `robot_vacuum` (Matter: RVC Operational State and RVC Run Mode): `start` starts cleaning, or
  carries on after a pause; `pause`; `return_to_dock` sends it back to charge. Each needs its
  feature; a robot set up for monitoring only reports them all `false`. `status`: `docked` (on
  the dock, charged), `charging`, `cleaning` (also on its way to clean a spot), `paused`,
  `returning` (to the dock), `idle` (off the dock and not cleaning, e.g. standing by or asleep),
  `error` (stuck or needs attention). Unknown device states are not mapped to any status (the
  state stays unknown). `cleaned_area_m2` and `cleaning_duration_s` describe the current or last
  cleaning. Its charge is a `battery` capability on the same endpoint; `low` is at 20 % or less.

## Units & Conventions
- Percent: integers `0–100`. Color temperature: Kelvin (not mireds). Hue: degrees `0–360`. Power: W. Energy: Wh. Temperature: °C. Durations: ms. Current: A.
- Time of day: `"HH:MM"`, 24-hour clock (`"00:00"`–`"23:59"`), the home's local time (Matter: minutes past midnight). Days of the week: `monday` … `sunday`.
- Cover position: `100` = fully open, `0` = fully closed. (Matter's lift percentage is inverted; the bridge converts.)
- Power sign follows the meter `role`'s natural direction: `grid` positive = importing, negative = exporting; `solar` positive = producing; `load`/`ev` positive = consuming; `house` (the whole home, sub-metered loads such as the EV charger included) positive = consuming; `battery` positive = discharging (supplying the home), negative = charging.
- Drivers convert vendor units (e.g. brightness `10–1000` → `0–100`). Rounding is the driver's job, never the frontend's.

## Commands
- Commands target one endpoint and one capability.
- Wire shape:
  ```json
  { "type": "command", "request_id": "c-42", "target": "living_room_fan/light",
    "command": { "capability": "level", "action": "set", "level": 50, "transition_ms": 300 },
    "confirmed": false }
  ```
- `confirmed` (default `false`) is `true` only after the user explicitly confirmed a sensitive command (see below).
- Lifecycle: the frontend shows the change as **pending** immediately, the Hub forwards it to the driver, and the pending state resolves when the driver reports the new state. If no confirmation arrives before the timeout (default 5 s; 20 s for slow commands such as `ev_charger.set_charging_schedule`, which may go through the internet), the UI reverts and shows an error. While a command is on its way, the control that sent it is busy and refuses to send it again.
- Saving a charger's hours (a slow command):
  ```json
  { "type": "command", "request_id": "c-44", "target": "car_charger/charger",
    "command": { "capability": "ev_charger", "action": "set_charging_schedule", "enabled": true,
                 "windows": [{ "start": "22:00", "end": "06:00", "days": ["saturday", "sunday"] }] },
    "confirmed": false }
  ```
- Result: the Hub answers every command with a `command_result`. `ok: true` means the device accepted it; `error` is present exactly when `ok` is `false`:
  ```json
  { "type": "command_result", "request_id": "c-42", "ok": false, "error": { "code": "rejected", "reason": "confirmation_required" } }
  ```
- Error `code`s: `unsupported`, `offline`, `timeout`, `rejected` (with `reason`: `not_allowed` for role/permission/time window, `confirmation_required` for a sensitive command sent without `"confirmed": true`), `device` (optional driver-specific `message` for diagnostics, never shown raw to users and never containing hosts, addresses or credentials).

### Sensitive Commands
`lock.unlock`, `lock.unlatch`, `trigger.fire`, `ev_charger.unlock` (anyone could charge afterwards), and `cover.open` / `cover.set_position` on endpoints that guard access to the home (`garage_door`, `gate`, `door`, `lock` — the types marked sensitive above) are **sensitive**:
- They require a role that allows them (Guests may be limited to specific targets and time windows).
- For example, unlocking a charger after the person confirmed it:
  ```json
  { "type": "command", "request_id": "c-43", "target": "car_charger/charger",
    "command": { "capability": "ev_charger", "action": "unlock" }, "confirmed": true }
  ```
- On mobile they require biometric confirmation (Face ID / fingerprint) before sending, and the request carries `"confirmed": true`.
- The Hub enforces this (role check, and `rejected` / `confirmation_required` without `confirmed`); the frontend only provides the confirmation UX.

## Events
- Events are instantaneous facts, not state: `doorbell.pressed`, `occupancy.detected`.
- Shape:
  ```json
  { "type": "event", "target": "front_doorbell/main", "event": "doorbell.pressed", "at": "2026-01-01T12:00:00Z" }
  ```
- Dashboards reference events as `{ "target": ..., "event": ... }`, e.g. to wake the panel.

## Availability
- Each device reports `online: bool` and `last_seen`. Offline devices keep their last state but widgets render as unavailable.

## WebSocket Messages (sketch)
- Server → client: `snapshot` (`devices`: all devices), `state` (`target`, `capability`, `state`), `event`, `availability` (`device_id`, `online`), `command_result` (`request_id`, `ok`, `error` when not ok), `dashboard_changed` (`revision`).
- Client → server: `command`.
- On connect, reconnect, or `RecvError::Lagged` on the Hub side, the client receives a fresh `snapshot`.
- `state` always carries the capability's full state, e.g. a charger waiting for its charging hours:
  ```json
  { "type": "state", "target": "car_charger/charger", "capability": "ev_charger",
    "state": { "status": "waiting", "waiting_for": "schedule", "power_w": 0.0, "session_energy_wh": 0.0,
               "current_a": 16.0, "auto_current": false, "locked": false, "schedule_enabled": true } }
  ```
  or a light:
  ```json
  { "type": "state", "target": "living_room_fan/light", "capability": "level", "state": { "level": 80 } }
  ```
- `availability`:
  ```json
  { "type": "availability", "device_id": "living_room_fan", "online": false }
  ```
- `dashboard_changed` is sent to every session when the dashboard was saved (from any screen or phone). `revision` is the new `ETag` of `GET /api/v1/dashboard` without the quotes; screens that are not being edited reload the dashboard unless they already show that revision. A screen in edit mode keeps its draft, and saving it then fails with `412` (see *Dashboard Editing*). It is not part of the snapshot: clients load the dashboard over HTTP on every (re)connect.
  ```json
  { "type": "dashboard_changed", "revision": "5f2c9a0e7b1d4c38" }
  ```
- Types are defined once in `vesta-core` and exported to TypeScript (`web/src/api/generated/`, regenerated by `cargo test -p vesta-core`).
- Hub handling of bad input:
  - A command for an unknown target, or for a capability the endpoint does not have, gets `unsupported`.
  - Malformed or unparseable client messages (no usable `request_id`) are dropped and logged at `debug`; the connection stays open.
  - `request_id` is limited to 64 characters; longer ids are treated as malformed.

## Dashboard Editing
The dashboard (`dashboard.json`) is edited from the UI only; people never see JSON. The Hub stores it and tells every screen when it changes.

- `GET /api/v1/dashboard` returns the dashboard with an `ETag` (a hash of its content).
- `PUT /api/v1/dashboard` replaces it. The body is a full `Dashboard` (`Content-Type: application/json`, at most 256 KiB) and the request must carry `If-Match` with the `ETag` the edit started from, so two people editing at once never silently overwrite each other.

| Status | Meaning |
|---|---|
| `200` | Saved. The body is the stored dashboard, `ETag` its new revision; every session gets `dashboard_changed`. |
| `400` / `422` | Not JSON / not a valid dashboard (`schema_version`, unique `snake_case` page ids, non-empty titles, `1..=12` columns, spans that fit the grid, well-formed targets). Unknown targets and widget types are accepted (a device may be offline, a newer screen may know the widget) and logged. |
| `412` | Stale: someone else saved first. The response carries the current `ETag`; the screen reloads and tells the person in plain words. |
| `415` | The body is not `application/json`. |
| `428` | `If-Match` is missing. |
| `500` | The Hub could not store it; nothing changed. |

- Storage is atomic: a temporary file next to `dashboard.json`, flushed to disk, then renamed over it. `vesta-hubd --dev` keeps edits in memory only (a restart resets the simulated home).
- Editing will be **Admin-only** once pairing and roles exist (the Hub checks the role before `PUT`, answering `403`).

## Rust Shape (sketch)

```rust
#[async_trait]
pub trait DeviceDriver: Send + Sync {
    /// Endpoints, types, capabilities and features. Reported by the driver.
    fn describe(&self) -> DeviceDescriptor;
    /// Execute a typed command on one endpoint.
    async fn execute(&self, endpoint: &EndpointId, cmd: Command) -> Result<(), DriverError>;
    /// Driver loop (polling or push subscription). Publishes state and events.
    async fn run(&self, sink: EventSink) -> Result<(), DriverError>;
}

pub enum Command {
    OnOff(OnOffCommand),               // On, Off, Toggle
    Level(LevelCommand),               // Set { level, transition_ms }
    ColorTemperature(ColorTemperatureCommand),
    Color(ColorCommand),
    Cover(CoverCommand),               // Open, Close, Stop, SetPosition { position }
    Fan(FanCommand),                   // SetSpeed { speed }, SetDirection { direction }
    Lock(LockCommand),                 // Lock, Unlock, Unlatch
    Trigger(TriggerCommand),           // Fire
    EvCharger(EvChargerCommand),       // Pause, Resume, SetCurrent { amps }, SetMode { mode }, Lock, Unlock,
                                       // SetSchedule { enabled }, SetChargingSchedule { enabled, windows }
    RobotVacuum(RobotVacuumCommand),   // Start, Pause, ReturnToDock
}
```

Drivers return `DriverError::Unsupported` for commands they cannot execute. Routing a frontend command is a single `match`; testing a driver is feeding it commands and asserting the mocked I/O.

`EventSink` is bound to one device and feeds the Hub's device runtime through a bounded channel (no update is dropped before the snapshot cache records it). The runtime then fans updates out to clients over the `EventBus` (`tokio::sync::broadcast`), where a lagging subscriber gets a fresh snapshot instead.

## Configuration Overrides
`config.json` holds driver connection settings and optional presentation overrides per endpoint. It never declares capabilities.

```json
{
  "id": "front_door_strike",
  "name": "Front Door",
  "driver": "shelly",
  "config": { "host": "192.168.1.51", "relay_index": 0, "mode": "pulse", "pulse_ms": 1500 },
  "endpoints": { "main": { "type": "door" } }
}
```

## Matter Mapping (reference)

| VESTA | Matter cluster |
|---|---|
| `on_off` | OnOff |
| `level` | LevelControl |
| `color_temperature`, `color` | ColorControl |
| `cover` | WindowCovering (closures in newer spec versions) |
| `fan` | FanControl |
| `lock` | DoorLock |
| `occupancy` | OccupancySensing |
| `contact` | BooleanState |
| `temperature` | TemperatureMeasurement |
| `humidity` | RelativeHumidityMeasurement |
| `power_meter` | ElectricalPowerMeasurement, ElectricalEnergyMeasurement |
| `ev_charger` charging hours (`charging_schedule`) | no equivalent (EnergyEVSE `ChargingTargetSchedules` are targets, not windows); a bridge exposes `schedule_enabled` only |
| `ev_charger` | EnergyEVSE (`State`, `SupplyState`, `SessionDuration`, `SessionEnergyCharged`, `MinimumChargeCurrent` / `MaximumChargeCurrent`, `CircuitCapacity` for the hardware maximum; `pause`/`resume` ↔ `Disable` / `EnableCharging`), EnergyEVSEMode for `mode` (Manual, TimeOfUse, SolarCharging), ElectricalPowerMeasurement for power and per-phase current; `lock` has no EVSE equivalent |
| `battery` | PowerSource |
| `robot_vacuum` | RVC Operational State (`docked` ↔ Docked, `charging` ↔ Charging, `cleaning` ↔ Running, `paused` ↔ Paused, `returning` ↔ SeekingCharger, `idle` ↔ Stopped, `error` ↔ Error; `start`/`pause`/`return_to_dock` ↔ Resume / Pause / GoHome), RVC Run Mode (Idle, Cleaning) |
| `on_off.features.read_only` | no Matter equivalent (a bridge exposes such an endpoint without the OnOff commands) |
| `trigger`, `camera`, `doorbell` | to be decided (depends on Matter version support) |

## Versioning
- Adding a capability, a feature, or an optional state field is backwards compatible.
- Renaming or removing anything is a breaking change and bumps `schema_version`.

## Driver Guidance
- Report features honestly; if the hardware can't report position, set `position: false`.
- Vendor-specific mappings (e.g. Tuya data points per product category) stay inside the driver crate, with fixtures for each supported category.
