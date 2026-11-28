# vesta-driver-v2c

VESTA driver for **V2C Trydan** EV chargers, over the charger's local HTTP API. It is the only
crate in VESTA that knows the brand: the Hub and the wall panel see an `ev_charger` endpoint and
`power_meter` endpoints (see `docs/architecture/capabilities.md`).

Everything runs on the local network, with one optional exception: with a `cloud_api_key`, the
household can see and change the charger's **charging hours** from the panel. They are stored in
the charger through V2C's cloud at the moment someone saves them, and read from it when someone
opens the hours editor (at most once per 30 s); never on a timer (see
[Charging hours](#charging-hours)).

Enabled with the `v2c` feature of `vesta-drivers` (on in `vesta-hubd` and in the app's `hub`
feature). Config key: `"driver": "v2c"`.

## What the household gets

| Endpoint | Type | What it shows | What it can do |
|---|---|---|---|
| `charger` | `ev_charger` | status in plain words, power to the car, this charge (energy, time), set and measured current, mode, locked, charging hours (on/off; with a `cloud_api_key`, the hours themselves, as last read or saved, with where from and when) | pause / resume, charging current, solar modes (solar installs), lock / unlock, charging hours on / off; with a `cloud_api_key`, read the hours set on the charger and change them (start, end, days; two time windows) |
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
    "charger_id": "ABC123",
    "cloud_api_key": "<only in the Hub's own config.json>"
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
| `cloud_api_key` | — | Optional. A key for V2C's cloud API, **only** to read and save charging hours from the panel. A secret: see below. |

## Charging hours

The household can program charging from the panel: start time, end time, days, on/off. The
design follows AGENTS.md rule 7 (safe degradation): **the hours live in the charger**, which
follows them by itself, so charging keeps to them when VESTA, the panel or the internet are
down.

### What the APIs can do (research, 2026-11)

| | Local HTTP API | V2C cloud API |
|---|---|---|
| Switch the charger's timers on / off | yes: `Timer` (`/write/Timer=0/1`, read in `RealTimeData`) | — (not used) |
| Read the hours (start, end, days) | **no**: no key or Modbus register holds them | yes: `GET /device/reported` (documented endpoint, undocumented fields) |
| Write the hours | **no** | yes: `POST /device/timer` |
| Needs | the home network | internet, a V2C account and an API key |

- **Local API**: V2C's own API sheet (tabs "HTTP GET Requests", reviewed 14/07/26, and "RS485
  V2C Modbus", reviewed 04/05/26, linked from `github.com/V2Charge/Trydan_Modbus_TCP`) has only
  on/off flags for the timers: HTTP `Timer` ("1 Timers are ON; 0 Timers are OFF"), Modbus
  `WRITE_ADDRESS_PROMGRAM` 6012 and `READ_ADDRESS_PROMGRAM` 3020. No key or register holds start
  time, end time or days. The firmware notes up to 2.5.2
  (`v2charge.com/es/actualizaciones-trydan/`) add nothing about them to the HTTP API.
  `pytrydan` (MIT) and Home Assistant's `v2c` integration only switch `Timer` ("set on the
  mobile application"); evcc reads `Timer` and runs its own scheduling.
  A real Trydan on firmware 2.5.2 (read-only check of `RealTimeData`, 2026-11) reports `Timer`
  and no field with hours or days.
- **Cloud API** (`api.v2charge.com`, source `github.com/V2Charge/API-doc-v2c`, `openapi.yaml`):
  base `https://v2c.cloud/kong/v2c_service`, header `apikey: <key>` (missing or wrong: `401`),
  **1000 calls per day** per user (`RateLimit-Limit` / `RateLimit-Remaining` headers).
  `POST /device/timer?deviceId=<ID>&timerId=<n>` with
  `{"timeStart": "22:00", "timeEnd": "06:00", "daysOfWeek": "123"}` ("1=Monday … 7=Sunday");
  answers `200 "Command executed successfully"` (no body schema), `400`, `401`, `500`. No
  documented way to read, delete or disable one timer.
- **Reading the hours**: `openapi.yaml` documents `GET /device/reported` ("Retrieve Device Last
  Reported Status (Shadow)": "the last known status and data reported by the device", one required
  query parameter `deviceId`, answers `200`, `400`, `401`, `500`) but **no response schema**, so
  its fields are undocumented. V2C's own Telegram bot (`github.com/V2Charge/Bot-Telegram-V2C`,
  re-checked 2026-11): `src/api.js` sends exactly `GET /device/reported` with `params: { deviceId }`
  and the `apikey` header to `https://v2c.cloud/kong/v2c_service`, and uses the parsed JSON object
  as is; `src/callbacks/ScheduleInfo.js` reads `hourstart_00`, `minutestart_00`, `hourend_00`,
  `minuteend_00`, `daysofweek_00` (timer 1) and the same with `_01` (timer 2), treats
  `daysofweek = "8"` as "not set", reads the other `daysofweek` digits `1`…`7` as Monday…Sunday,
  and shows "nothing scheduled" when `programed` is `0`. The community `HomeAssistant-V2C-Cloud`
  integration (`custom_components/v2c_cloud/v2c_cloud.py`) notes that "some endpoints reply with
  json encoded as text/plain" and parses a `/device/reported` answer that arrives as a JSON
  string a second time; it never retries `429` ("every retry consumes quota").
- **Two timers**: "You can create up to 2 timers" (V2C's support pages on scheduling and on
  configuring V2C Cloud).
- **The hours are kept in the charger**: they can be programmed over Bluetooth alone, with no
  internet path, and the charger's top button toggles scheduled charging (V2C's V2C Cloud
  configuration guide; firmware 1.3.7 "Atajos en los botones: Lock y Programación de Carga").
  No V2C page says in so many words that timers keep running offline; this is an inference from
  those facts. The charger keeps its own clock (NTP, automatic time zone since firmware 1.5.2).
- **Field reports** (facts only): the community `HomeAssistant-V2C-Cloud` integration
  (`github.com/samuelebistoletti/HomeAssistant-V2C-Cloud`, MIT) notes a malformed
  `/device/timer` request "still returns HTTP 200" (its PR #55), never retries `429` (each retry
  spends quota), and takes the cloud's `deviceId` from `/pairings/me`, using it as the local `ID`.

### What VESTA relies on in the cloud's answers

Only these, and nothing else in the answer:

| Request | Field | Used as |
|---|---|---|
| `GET /device/reported?deviceId=<ID>` | `hourstart_0N`, `minutestart_0N`, `hourend_0N`, `minuteend_0N` (N = 0, 1; numbers or numeric strings) | start and end of timer N |
| | `daysofweek_0N` (string or number of digits `1`–`7`, Monday = 1) | its days; `"8"`: timer N is not set |
| | the object as JSON, or wrapped in a list or a `data` field, or as text inside a JSON string | (all accepted) |
| `POST /device/timer?deviceId=<ID>&timerId=<N>` | any `2xx` | the write was accepted (then checked by the read above) |

`programed` and every other field are ignored: whether the hours are on comes from the local
`Timer`. If the timer fields are missing, the read fails and nothing changes.

### The design

- **Saving** (`set_charging_schedule`): VESTA writes both timers (`POST /device/timer`,
  `timerId` 0 and 1; one window goes into both, since a timer cannot be cleared), reads them back
  (`GET /device/reported`) because a `200` does not prove the charger took them, then switches the
  schedule on or off **locally** (`/write/Timer`). Three cloud calls per save, well within the
  daily limit.
- **Reading** (`refresh_charging_schedule`, `refreshable: true`): the panel asks for it when
  someone opens the hours editor, and only then; nothing reads the cloud on a timer. One
  `GET /device/reported`, the same request as the read-back. **At most once per 30 s per
  charger**: asking sooner answers like the last read did (also when it failed, so a person
  tapping again does not spend the daily quota); a save's read-back counts as a read. The
  charger's hours, also those changed in V2C's app, then show on the panel. Nothing is written
  to the charger.
- **One cloud request at a time**: a read never races a save.
- **Where the hours came from**: the panel shows the hours with their source and time ("Hours
  read from the charger just now", "saved from this screen"), from `state.charging_schedule.origin`.
- **Kept across restarts**: the last known hours (windows, source, time) are kept in the
  device's state (one JSON file per device in the Hub's state folder; see `vesta-hubd
  --state-dir`) and restored at start, marked as the last known ones (`origin.restored`), with no
  cloud call. Only with a `cloud_api_key` and not `read_only`. The state never holds the key, the
  charger's ID or its address (a test checks); unreadable state is ignored.
- **Daily operation stays local**: reading the charger, pause/resume, current, lock, and switching
  the hours on and off never touch the cloud.
- **The cloud requests are a closed list** (`cloud::CloudRequest`: the timer write and
  `/device/reported`, for the read-back and the read on request). There is no way to reach the
  cloud's endpoints for the contracted power, dynamic power control, current limits or anything
  else; a test sends every command (and a read after each) and checks methods, paths, query
  keys and bodies.
- **Errors**: no internet, a wrong key (`401`/`403`), the daily limit (`429`) or a charger that did
  not take the hours: the save fails, the panel says so in plain words, and nothing is switched on
  the charger. Each cloud request has 5 s and the cloud part 13.5 s; the whole save, local write
  and fresh reading included, ends within 18 s (the Hub gives slow commands 20 s). A read that
  fails (same reasons, or an answer without the timer fields) changes nothing: the editor still
  opens with the last known hours and a gentle note asking whether the home's internet works.
  Errors never quote the key or the charger's ID.
- **Without a key**, the panel shows the schedule's on/off switch only, and the hours are set in
  V2C's app as before.

If VESTA, the panel or the internet stop working, the charger simply goes on following the hours
stored in it, with its schedule on or off as last set, and keeps its own power control. Nothing
in VESTA is needed for that.

**Not chosen: a schedule run by VESTA** (pausing and resuming the charger at the right times, as
evcc does). It needs no cloud, but if VESTA is off the charger would just follow its own
settings: with its timers off it charges whenever a car is plugged in, possibly at peak hours.
That breaks rule 7, so it is not implemented.

### Enabling it (the household's installer)

1. On the **v2c.cloud** website (not the phone app), sign in with the charger's V2C account, open
   **User** (top left) → **API** → **Get token** (V2C support page "Get token ID API REST
   cloud").
2. Put it in the Hub's own `config.json`, in the charger's `config`, as `"cloud_api_key"`. That
   file never goes into the repository; `vesta-hubd` reads it at start.
3. Restart the Hub. The charger's "everything" dialog now has **Change hours**.

The key is a secret: whoever has it controls the charger through V2C's cloud. VESTA never logs
it, never shows it on a screen or in the API, never keeps it in the device's state, and config
and cloud errors never quote it. If it leaks, make
a new one on v2c.cloud (the old one stops working).

### Not known yet (checked in the supervised live test)

- How quickly `/device/reported` (the cloud's "shadow" of the charger) reflects hours changed in
  V2C's app, and whether hours set over Bluetooth alone reach it at all.

- Whether the cloud numbers the timers 0/1 or 1/2 (`timerId`). V2C's only example is `1`; the
  read-back fields end in `_00`/`_01`; a community client says "0 or 1". VESTA uses 0 and 1: if
  that is wrong, the first write fails and nothing changes, or the read-back catches it.
- Whether the cloud's `deviceId` is always the charger's local `ID` (every client seen treats them
  as the same).
- How the charger reads the days of a window past midnight (VESTA's model: the days it starts
  on).

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
  before the command is confirmed (within the Hub's 5 s). Saving and reading charging hours are
  the exceptions (cloud, then at most one local write; see [Charging hours](#charging-hours)).
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
The cloud gets a closed list too (`cloud::CloudRequest`): the timer write and
`/device/reported` (the read-back after a save, and the read when someone opens the hours
editor); another test sends every command and checks the cloud never gets anything else.

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
control it (pause, lock, change the current) with a browser. The optional `cloud_api_key` is a
real secret (see [Charging hours](#charging-hours)); it only ever goes to V2C's cloud, over
HTTPS (rustls, Mozilla's root certificates). VESTA adds no new exposure: it only
talks to the charger from the Hub on the local network, never opens the charger to the internet,
and the panel and phones reach the Hub through its own authenticated connection. The charger's
address and ID stay in the Hub's `config.json` and logs; the API never shows them. Keep the home
Wi-Fi password private (or put the charger on a separate network for devices).

## Not supported (yet)

- Showing hours changed in V2C's app before someone opens the hours editor (by design: the
  cloud is only read when someone asks).
- Phase switching (`ChargeMode`), LED brightness, the contracted power and the app's power
  profiles: see above.
- Lifetime energy counters: the local API only reports the current session.
