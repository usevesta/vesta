# vesta-driver-tuya

VESTA driver for **Tuya** devices (sold under many brands and set up with the Smart Life or
Tuya Smart app), over the devices' own local network protocol. No Tuya cloud is involved once a
device is set up: VESTA talks to it directly on the home network. It is the only crate in VESTA
that knows the brand: the Hub and the wall panel see `outlet`, `switch` and `robot_vacuum`
endpoints (see `docs/architecture/capabilities.md`).

Enabled with the `tuya` feature of `vesta-drivers` (on in `vesta-hubd` and in the app's `hub`
feature). Config key: `"driver": "tuya"`.

## The local key is a secret

Every Tuya device has a 16-character **local key**. Whoever has it (and is on the home Wi-Fi) can
control the device. VESTA keeps it in the Hub's `config.json` only:

- never commit that file, paste it in a chat or an issue, or show it in a screenshot;
- `vesta-hubd tuya-import` writes it readable by its owner only (mode `0600`);
- the driver never logs the key or the device ID, the API never exposes them, and config errors
  never quote them.

If a key leaks, remove the device in the Smart Life app and add it again: it gets a new key.

## What the household gets

| Tuya category | Endpoint (type) | What it shows | What it can do |
|---|---|---|---|
| `cz` (socket), `tdq` (on/off switch, e.g. plugs), `kg` (switch) | `main` (`outlet`; `switch` for `kg` and for `tdq` without metering). Several channels: `switch_1`, `switch_2`, … plus `power` | on/off; power (W), voltage (V), current (A) when it measures; lifetime energy when the device has such a counter | on / off |
| `sd` (robot vacuum) | `main` (`robot_vacuum`) | status in plain words (docked, charging, cleaning, paused, returning, idle, needs help), battery, area and time of the last clean | start / carry on, pause, back to the dock |

Anything else (lights, covers, sensors, …) is not shown yet: an unknown category exposes nothing
rather than a guess, and the hub logs it. `tuya-import` skips such devices and says so.

**Energy.** Many plugs only report `add_ele`: the energy used since the plug last reported to the
Tuya cloud, which the cloud adds up into its statistics. It goes back to zero on its own, so it is
not a total and VESTA does not show it as one. Plugs with a lifetime counter
(`total_forward_energy`) show it.

**Read-only.** With `"read_only": true`, VESTA shows the device but never sends it a command:
`on_off` reports `features.read_only`, the robot reports no features, the Hub refuses commands
and the driver refuses them again. Only session set-up, status queries and heartbeats are sent.

## Config

`vesta-hubd tuya-import` writes these entries; nobody needs to edit them by hand.

```json
{
  "id": "hall_lamp",
  "name": "Hall Lamp",
  "driver": "tuya",
  "config": {
    "device_id": "bf00000000000000000000",
    "local_key": "0123456789abcdef",
    "protocol_version": "3.5",
    "host": "192.0.2.20",
    "category": "cz",
    "mapping": {
      "1": { "code": "switch_1", "type": "Boolean", "values": {} },
      "22": { "code": "cur_power", "type": "Integer", "values": { "unit": "W", "scale": 1 } }
    },
    "read_only": false
  }
}
```

| Key | Default | Meaning |
|---|---|---|
| `device_id` | (required) | The device's ID. |
| `local_key` | (required) | 16 characters. Secret (see above). |
| `protocol_version` | (required) | `3.3`, `3.4` or `3.5`. When the device announces another version (firmware updates change it), the announced one is used and a warning is logged. |
| `host` | — | Last known IPv4 address: a hint only. |
| `port` | 6668 | The device's local port (only tests change it). |
| `category` | (required) | Tuya product category, e.g. `cz`. |
| `mapping` | `{}` | The device's data points by number: `code`, `type`, `values` (unit, scale, range), as tinytuya writes them. Values are scaled with them (`cur_voltage` 2272 with scale 1 is 227.2 V). |
| `read_only` | `false` | Monitoring only (see above). |
| `poll_interval_ms` | 10000 | Time between status queries, 5000–300000. |

## Getting the keys

Today: with [tinytuya](https://github.com/jasonacox/tinytuya)'s wizard, which reads the device
list (names, IDs, local keys, data points) from a free Tuya IoT developer account linked to the
Smart Life app (`python -m tinytuya wizard`, see its README). Then:

```sh
vesta-hubd tuya-import devices.json --out config.json          # adds them; keeps other devices
vesta-hubd tuya-import devices.json --out config.json --read-only --only "Hall lamp"
vesta-hubd tuya-import devices.json --out config.json --snapshot snapshot.json   # tinytuya scan results
vesta-hubd tuya-import devices.json --out config.json --check   # then read each one's status once
```

The importer listens for the devices' broadcasts for 12 s (`--listen`) to learn each one's
protocol version and address; it sends nothing to them. A device that does not announce itself
(some robot vacuums only answer the app's discovery requests) is taken from `--snapshot`
(`python -m tinytuya scan`) or skipped. Running it again updates keys and versions and keeps the
names, rooms and presentation set since.

Roadmap: adding devices by scanning the QR code the Smart Life app shows ("link a third-party
service"), so no developer account and no tinytuya are needed.

## How it behaves

- **Finding the device.** Devices broadcast their ID, address and version every few seconds on
  UDP 6666 / 6667 / 7000. One passive listener per hub (shared by every Tuya device; sockets with
  `SO_REUSEADDR`/`SO_REUSEPORT`, so another scanner can listen too) keeps the latest address per
  ID. The driver prefers an address heard in the last 5 minutes, then `host`, then waits for a
  broadcast. A broadcast can at worst point VESTA at a wrong address: the session only starts
  when the device proves it knows its local key (3.4, 3.5) or answers with its own ID in data
  readable with that key (3.3).
- **One session, kept open.** TCP to port 6668; status query on connect and every
  `poll_interval_ms`; a heartbeat when nothing else was sent for 10 s; data points the device
  pushes by itself (`STATUS`, e.g. after its button was pressed) are taken as they come. Only
  changes are published.
- **Not responding.** No message for ~35 s ends the session. A dropped session is retried after
  1 s; two failed attempts in a row mark the device offline. It keeps trying with a backoff up to
  60 s and comes back by itself. A wrong key or another device at the address never shows as
  online.
- **Commands** set exactly the data points of `model.rs` (`switch_1: true`, `pause: true`, …),
  one command at a time, and are confirmed by the device's answer or its report of the new
  values within 4.5 s (the Hub gives up at 5 s). A command the device cannot take now is refused
  at once and never kept for later.
- **After a command** that worked (whichever came first, the answer or the report), the status
  is asked again 1 s, 2.5 s and 5 s later: a plug reports its switch at once, but its power,
  voltage and current catch up a moment later, and waiting for the next poll (10 s) left the old
  reading on screen. These are ordinary status queries; nothing else is sent. Robot: `start` sets `power_go: true`, or `pause: false` when
  it is paused; `pause` sets `pause: true` (or `power_go: false`); back to the dock sets
  `switch_charge: true` (or `mode: chargego`).

## Protocol notes

All integers big-endian. Commands used: `0x03`–`0x05` key negotiation, `0x07` set (3.3), `0x08`
pushed status, `0x09` heartbeat, `0x0a` status query (3.3), `0x0d` set (3.4/3.5; also the status
query of 3.3 devices that answer `data unvalid`, with every data point `null`), `0x10` status
query (3.4/3.5), `0x13`/`0x23` broadcasts.

| | 3.3 | 3.4 | 3.5 |
|---|---|---|---|
| Frame | `000055AA` seq cmd len · body · CRC-32 · `0000AA55` | same, HMAC-SHA256 instead of CRC | `00006699` `0000` seq cmd len · 12-byte nonce · ciphertext · 16-byte tag · `00009966` |
| `len` counts | body, check, suffix | body, HMAC, suffix | nonce, ciphertext, tag |
| Body | AES-128-ECB (PKCS#7), local key | AES-128-ECB, session key | AES-128-GCM, session key, the 14 header bytes after the prefix as associated data |
| Version header (`3.x` + 12 bytes) | before the ciphertext, in the clear | inside the ciphertext | inside the ciphertext |
| From the device | 4-byte return code before the body | same | return code at the start of the plaintext |
| Status query | `{gwId, devId, uid, t: "secs"}` | `{}` | `{}` |
| Set | `{devId, uid, t: "secs", dps}` | `{protocol: 5, t: secs, data: {dps}}` | same as 3.4 |

The version header goes on every command except queries, heartbeats and the negotiation.

**Key negotiation (3.4, 3.5)**, under the local key: the client sends a random 16-byte nonce
`a`; the device answers its nonce `b` and `HMAC-SHA256(local_key, a)`, which the client checks;
the client sends `HMAC-SHA256(local_key, b)`. Session key: `a XOR b` encrypted with the local key,
with AES-ECB (3.4) or AES-GCM with `a[0..12]` as nonce, ciphertext only (3.5). Every 3.5 frame
gets a fresh random nonce from the operating system.

**Broadcasts**: 55AA with a return code; JSON in the clear (3.1, port 6666) or AES-ECB under the
well-known key `MD5("yGAdlopoPVldABfn")` (port 6667); 6699 AES-GCM under the same key (3.5). They
identify devices; they protect nothing.

Seen on real devices (read-only: session set-up and status queries only, while writing this
driver): three plugs (`tdq`, 3.5) broadcast every few seconds on 6667 in 6699 frames and answer
`0x10` with all their data points; a robot vacuum (`sd`, 3.5) answers the same way but did not
broadcast by itself in two minutes, so it is reached at its known address. The plugs reported
`add_ele` of 2–4 (0.002–0.004 kWh) while switched off, consistent with a counter of energy not
yet reported to the cloud.

### Sources

- [tinytuya](https://github.com/jasonacox/tinytuya) (MIT): frame layouts, command codes, the
  version header rules, the negotiation and session-key derivation, payload templates per
  version, the UDP key and ports, the `data unvalid` fallback.
- Tuya's standard data point codes for sockets (`cz`), switches (`kg`, `tdq`) and robot vacuums
  (`sd`), as listed in each device's `mapping` (from the Tuya cloud through tinytuya); `add_ele`
  is described there as "added electricity" and summed by the cloud's statistics.
- The Matter 1.2+ RVC Operational State and RVC Run Mode clusters, for the robot vacuum model.

Facts were read for this driver; no code was copied.

## Not supported (yet)

- Protocol 3.1 and 3.2, devices behind a Tuya gateway (Zigbee/BLE sub-devices), battery devices
  that sleep (most sensors).
- Lights (`dj`), covers (`cl`), fans, sensors and other categories.
- Sending the app's UDP discovery request to find devices that do not broadcast.
- The robot's maps, rooms, suction and mop settings, consumables (`edge_brush`, `filter`, …).
- Energy totals for plugs that only report `add_ele` (see above).
- On Android, broadcasts may need a Wi-Fi multicast lock the app does not take yet: the panel
  then reaches devices at their configured address only.
