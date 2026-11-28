# vesta-driver-v2c

VESTA driver for **V2C Trydan** EV chargers, over the charger's local HTTP API. It is the only
crate in VESTA that knows the brand: the Hub and the wall panel see an `ev_charger` endpoint and
`power_meter` endpoints (see `docs/architecture/capabilities.md`).

Enabled with the `v2c` feature of `vesta-drivers` (on in `vesta-hubd` and in the app's `hub`
feature). Config key: `"driver": "v2c"`.

## What the household gets

| Endpoint | Type | What it shows | What it can do |
|---|---|---|---|
| `charger` | `ev_charger` | status in plain words, power to the car, this charge (energy, time), set and measured current, mode, locked, charging hours | pause / resume, charging current, solar modes (solar installs), lock / unlock, charging hours on / off |
| `grid` | `meter` (`grid`) | import (+) / export (−), contracted power as `limit_w` | — |
| `house` | `meter` (`house`) | the whole home's consumption, car included | — |
| `solar` | `meter` (`solar`) | solar production (only with `meters.solar`) | — |
| `battery` | `meter` (`battery`) | home battery (only with `meters.battery`) | — |

## Config

```json
{
  "id": "car_charger",
  "name": "Car Charger",
  "room": "garage",
  "driver": "v2c",
  "config": {
    "host": "192.0.2.10",
    "poll_interval_ms": 3000,
    "request_timeout_ms": 3000,
    "read_only": false,
    "meters": { "grid": true, "house": true, "solar": false, "battery": false },
    "rediscover": true,
    "charger_id": "ABC123"
  }
}
```

| Key | Default | Meaning |
|---|---|---|
| `host` | (required) | IPv4 address or host name, optionally `:port`. No scheme, no path. |
| `poll_interval_ms` | 3000 | Time between readings, 2000–60000. The charger answers slowly (up to ~2 s). |
| `request_timeout_ms` | 3000 | Per request, 500–4000 (below the Hub's 5 s command timeout). |
| `read_only` | `false` | Monitoring only: every control is hidden and no command is ever sent. |
| `meters` | grid + house | Which readings to show. The charger cannot tell whether its clamp, solar panels or a battery exist. `solar` also enables the solar charging modes. |
| `rediscover` | `true` | Follow the charger to a new address (see below). |
| `charger_id` | — | The charger's ID (shown in its app). Optional: learned from the first reading. |

## How it behaves

- **Reading**: `GET /RealTimeData` every `poll_interval_ms`, one request at a time, a fresh
  connection each time (no keep-alive, no proxy, no redirects, answers capped at 64 KiB).
  Parsing is tolerant: numbers may come as ints, floats or numeric strings, unknown fields are
  ignored, missing ones stay unknown (never invented), and a repeated key keeps the last value.
- **Only changes are published**, per endpoint.
- **Not responding**: three failed readings in a row mark the charger offline (one drop is
  normal on a Wi-Fi extender). It keeps trying with a backoff up to 30 s and comes back by
  itself.
- **Rediscovery**: after 60 s without answers, if the charger's ID is known, the driver asks the
  other addresses of the charger's last `/24` network and of this machine's current one (only
  `GET /RealTimeData`, 16 at a time, 1.5 s timeout, at most every 10 minutes). Only a charger
  reporting the same `ID` counts; the new address is logged.
- **Identity**: if a different charger answers at the address (another ID), its readings are
  ignored and commands are refused at once, so VESTA never controls the wrong charger.
- **Commands** send exactly one write, wait at least 1 s after the previous write (some firmware
  mixes up writes sent together), then read the charger again so the screen shows the result
  before the command is confirmed (within the Hub's 5 s).
- **Meter errors**: while `SlaveError` is not 0 the clamp/meter readings are not real; the
  meters are not updated (logged once). The charger's own power control deals with it.

## Status mapping

| `ChargeState` | Other fields | VESTA status |
|---|---|---|
| 0 | — | `disconnected` (no car) |
| 1 or 2 | `Paused=1`, or `DynamicPowerMode=5` (stop) | `paused` |
| 1 or 2 | `Locked=1` | `connected` (+ `locked`) |
| 2 | — | `charging` |
| 1 | `DynamicPowerMode=3` (solar only) | `waiting` for `solar` |
| 1 | dynamic control on, grid import + minimum current would exceed `ContractedPower` | `waiting` for `power` |
| 1 | `Timer=1` | `waiting` for `schedule` |
| 1 | — | `connected` |
| 3–6, anything else | — | `error` |

The three `waiting` reasons are inferences (the API has no "why"): solar-only mode stops without
surplus; the power estimate uses `MinIntensity × VoltageInstallation`; with the schedule on and a
car that is not charging, the most likely reason is being outside the charging hours (a full car
looks the same).

## API facts used

Plain HTTP on port 80, **no authentication**. Every call is a `GET`, writes included.

| Request | Answer |
|---|---|
| `GET /RealTimeData` | one JSON object with every field below |
| `GET /write/<Key>=<Value>` | `200` with body `OK` when accepted (anything else is treated as an error) |
| `GET /read/<Key>` | one value as plain text (firmware ≥ 2.4.0; not used) |

Fields read (units as reported):

| Field | Meaning | Used for |
|---|---|---|
| `ID`, `FirmwareVersion` | charger ID, firmware (some 2.4.x firmware sends `FirmwareVersion` twice) | identity, logs |
| `ChargeState` | 0 A no car, 1 B car connected, 2 C charging, 4 F system fail, 5 E pilot/ground fault, 6 D ventilation required (3 is "ventilation required" in the cloud API) | status |
| `Paused` | 0 / 1 | status, pause/resume |
| `Locked` | 0 unlocked / 1 locked (blocked for any charging) | `locked`, lock/unlock |
| `Timer` | 0 / 1: the charger's schedules are off / on | `schedule_enabled` |
| `Dynamic` | 0 / 1: dynamic power control off / on | `auto_current`, status |
| `PauseDynamic` | 0 modulating / 1 not modulating | `auto_current` |
| `DynamicPowerMode` | 0 power profiles from the app (time slots), 1 deprecated (ignored), 2 solar + minimum from the grid, 3 solar only, 4 grid + solar (as fast as the contracted power allows), 5 stop | `mode` |
| `ChargeMode` | phases: 0 single-phase, 1 three-phase, 2 mixed | how many phases to report |
| `ChargePower` | W to the car | `power_w` |
| `ChargeEnergy` | kWh this session | `session_energy_wh` |
| `ChargeTime` | s this session | `session_duration_s` |
| `Intensity` | A offered to the car (reads 0 while paused) | `current_a` |
| `MinIntensity`, `MaxIntensity` | A, limits of the offered current | `min_current_a`, `max_current_a`, clamping |
| `IntensityMeasure_L1..L3` | A measured per phase (firmware ≥ 2.5.0) | `phase_current_a` |
| `VoltageInstallation` | V | minimum-power estimate |
| `GridPower` | W from the grid (firmware ≥ 2.5.0; undocumented, sign assumed + import) | `grid` meter; older firmware: `HousePower − FVPower` |
| `HousePower` | W, the home's consumption | `house` meter |
| `FVPower` | W, solar production | `solar` meter |
| `BatteryPower` | W | `battery` meter (sign **not documented**; assumed + discharging) |
| `ContractedPower` | W, −1 when not used | `grid.limit_w` (read only) |
| `SlaveError` | 0 OK; 1–10 communication, reading, slave, waiting Wi-Fi, waiting communication, wrong IP, slave not found, wrong slave, no response, clamp not connected; more codes exist | hides meter readings |
| `ReadyState`, `LightLED`, `LogoLED`, `SSID`, `IP`, `SignalStatus`, `VoltageMeasure_*` | not used | — |

### Sources

- V2C's own API spreadsheet, tabs "HTTP GET Requests" (reviewed 2026-07) and "RS485 V2C
  Modbus", linked from V2C's GitHub (`github.com/V2Charge/Trydan_Modbus_TCP`): endpoints, write
  answer, fields, `ChargeState`, `Locked`, `Timer`, `Dynamic`, `PauseDynamic`,
  `DynamicPowerMode`, `SlaveError`, `ChargeMode`.
- V2C support pages on dynamic power control and its modes (`v2charge.com/support/power-control/`)
  and the Trydan firmware release notes (`v2charge.com/es/actualizaciones-trydan/`).
- `pytrydan` (`github.com/dgomes/pytrydan`, MIT): the `OK` answer to writes, value ranges
  (Intensity 6–32 A), the corrected order of `DynamicPowerMode` 2/3 and the deprecated mode 1,
  the corrected meaning of `PauseDynamic`, extra `SlaveError` codes.
- Home Assistant's `v2c` integration (`home-assistant/core`, `homeassistant/components/v2c/`,
  Apache-2.0): which settings are exposed and their ranges; it does not expose the contracted
  power or the power mode.
- Field reports in `evcc-io/evcc` pull requests and issues (facts only; that charger module is
  not under an open license): `Intensity` is ignored while `Dynamic=1` and `PauseDynamic=0`;
  `ChargeState` stays 2 after `Paused=1` on 2.5.0; writing `Locked=1` resets the session
  counters and on one car released the cable; `GridPower` and per-phase fields appear in 2.5.0.

Facts were read for this driver; no code was copied from any of these projects.

## What VESTA never does, and why

AGENTS.md rule 7: VESTA never takes over a safety function a device already handles. The charger
keeps the home under its contracted power by itself; VESTA must not weaken that, even by
accident. The writes the driver can send are a closed list (`api::Write`): `Paused`, `Locked`,
`Timer`, `Intensity` and `DynamicPowerMode` 2/3/4. A test sends every command, with every
combination of charger settings and thousands of currents, and checks nothing else ever leaves.

| Never written | Why |
|---|---|
| `ContractedPower` | the limit the charger protects; set by the household/installer |
| `Dynamic`, `PauseDynamic` | turning off (or pausing) the charger's own power control |
| `MinIntensity`, `MaxIntensity` | installation limits (the circuit's rating); raising them could overload the wiring |
| `ChargeMode`, `VoltageInstallation` | installation settings (phases, voltage) |
| `DynamicPowerMode` 0, 1, 5 | 0 turns the app's time-slot power profiles on/off: they may hold different contracted powers per time of day. VESTA only switches between the solar modes (2, 3, 4), and only when the charger is already in one of them; otherwise it says the mode is changed on the charger itself. |
| `LightLED`, `LogoLED` | cosmetic; not part of the charger's job in VESTA's model (could become a `light` endpoint) |

Consequences the household sees:

- **Charging current**: while the charger's dynamic control is modulating (`Dynamic=1`,
  `PauseDynamic=0`, the usual setup with a clamp), the charger chooses the current itself and
  ignores `Intensity`. VESTA shows the current with "the charger sets it by itself" and refuses
  `set_current` instead of sending a write that would be ignored. When the control is off or
  paused (from the charger's app), the slider works, clamped to `MinIntensity..MaxIntensity`
  and rounded to whole amps.
- **Unlocking is sensitive**: it lets anyone charge, so it needs an authorized role and an
  explicit confirmation (`confirmed: true`); locking does not.

## Security note

The charger's local API has **no password**: anyone on the home Wi-Fi can already read it and
control it (pause, lock, change the current) with a browser. VESTA adds no new exposure: it only
talks to the charger from the Hub on the local network, never opens the charger to the internet,
and the panel and phones reach the Hub through its own authenticated connection. The charger's
address and ID stay in the Hub's `config.json` and logs; the API never shows them. Keep the home
Wi-Fi password private (or put the charger on a separate network for devices).

## Not supported (yet)

- Editing the charging schedule (start/end times): the local API only switches the schedule on
  and off (`Timer`). Schedules are edited in V2C's app (its cloud API has timer endpoints; VESTA
  does not use any cloud).
- Phase switching (`ChargeMode`), LED brightness, the contracted power and the app's power
  profiles: see above.
- Lifetime energy counters: the local API only reports the current session.
