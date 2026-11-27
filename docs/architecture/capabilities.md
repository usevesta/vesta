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
| `ev_charger` | `ev_charger`, `power_meter` | no |

## Capability Catalog

| Capability | Features | State | Commands | Events | Phase |
|---|---|---|---|---|---|
| `on_off` | — | `on: bool` | `on`, `off`, `toggle` | — | v1 |
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
| `power_meter` | `role` (`grid`, `solar`, `load`, `ev`), `bidirectional: bool` | `power_w`, `energy_wh`, `energy_exported_wh?` | — | — | v2 |
| `ev_charger` | `min_current_a`, `max_current_a`, `modes` (`manual`, `solar`, `scheduled`) | `status: disconnected \| connected \| charging \| paused \| error`, `power_w`, `session_energy_wh`, `current_a`, `mode` | `pause`, `resume`, `set_current { amps }`, `set_mode { mode }` | — | v2 |
| `battery` | — | `percent: 0–100`, `low: bool` | — | — | v2 |

Notes:
- `on_off` + `level`: `level.set` on an endpoint that is off turns it on (like Matter's *MoveToLevelWithOnOff*). Dragging a slider must light the lamp.
- `cover` without `position` (e.g. many garage doors) only offers `open` / `close` (and `stop` if supported). `open` reports the end-stop state when a contact sensor exists.
- `power_meter.role` describes *where* the clamp sits, which only the installer knows. Drivers may default it; users set it during setup. It is the one feature users may override.
- `ev_charger.set_current` is always clamped to `max_current_a`. The device's own limits (e.g. contracted-power control) always prevail — VESTA never replaces them (AGENTS.md rule 7).

## Units & Conventions
- Percent: integers `0–100`. Color temperature: Kelvin (not mireds). Hue: degrees `0–360`. Power: W. Energy: Wh. Temperature: °C. Durations: ms. Current: A.
- Cover position: `100` = fully open, `0` = fully closed. (Matter's lift percentage is inverted; the bridge converts.)
- Power sign follows the meter `role`'s natural direction: `grid` positive = importing, negative = exporting; `solar` positive = producing; `load`/`ev` positive = consuming.
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
- Lifecycle: the frontend shows the change as **pending** immediately, the Hub forwards it to the driver, and the pending state resolves when the driver reports the new state. If no confirmation arrives before the timeout (default 5 s), the UI reverts and shows an error.
- Result: the Hub answers every command with a `command_result`. `ok: true` means the device accepted it; `error` is present exactly when `ok` is `false`:
  ```json
  { "type": "command_result", "request_id": "c-42", "ok": false, "error": { "code": "rejected", "reason": "confirmation_required" } }
  ```
- Error `code`s: `unsupported`, `offline`, `timeout`, `rejected` (with `reason`: `not_allowed` for role/permission/time window, `confirmation_required` for a sensitive command sent without `"confirmed": true`), `device` (optional driver-specific `message` for diagnostics, never shown raw to users and never containing hosts, addresses or credentials).

### Sensitive Commands
`lock.unlock`, `lock.unlatch`, `trigger.fire`, and `cover.open` / `cover.set_position` on endpoints that guard access to the home (`garage_door`, `gate`, `door`, `lock` — the types marked sensitive above) are **sensitive**:
- They require a role that allows them (Guests may be limited to specific targets and time windows).
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
- `state` always carries the capability's full state:
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
    EvCharger(EvChargerCommand),       // Pause, Resume, SetCurrent { amps }, SetMode { mode }
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
| `ev_charger` | EnergyEVSE |
| `battery` | PowerSource |
| `trigger`, `camera`, `doorbell` | to be decided (depends on Matter version support) |

## Versioning
- Adding a capability, a feature, or an optional state field is backwards compatible.
- Renaming or removing anything is a breaking change and bumps `schema_version`.

## Driver Guidance
- Report features honestly; if the hardware can't report position, set `position: false`.
- Vendor-specific mappings (e.g. Tuya data points per product category) stay inside the driver crate, with fixtures for each supported category.
