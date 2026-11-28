# VESTA Roadmap

Status: **pre-alpha, design phase.** Nothing here is a promise; it is the current plan.

## Phases

### v1 — One home, local only
Goal: a real home controlled from a wall tablet and phones on the same network.
- Hub on an Android wall tablet (`hub` feature). `vesta-hubd` on macOS/Linux is the development loop from day one.
- Tuya local driver: lights, switches, blinds, garage door, fans.
- Pairing with a QR code and per-home device keys (LAN only).
- Dashboard rendered from `dashboard.json`, basic in-app editor, themes, kiosk basics.
- Safe degradation from day one (see `AGENTS.md`, rule 7).

### v1.1 — Away from home
- Remote access over iroh (NAT traversal, relay fallback).
- Multiple homes per client app.
- Push notifications via ntfy.

### v2 — Doorbell, door access & energy
- Video doorbell: live WebRTC video, two-way audio, ring event, wake the panel.
- Door opening: motorized smart lock (stateful lock) or electric strike (momentary trigger), with biometric confirmation.
- Energy: EV chargers with a local API (V2C Trydan driver and the energy-flow charger widget: done), power meters (e.g. Shelly Pro EM).
- Full Android kiosk: Device Owner provisioning, LockTask, silent OTA.
- Shelly driver.

### Later
- Headless hub as a product (Docker image, `.deb`) + iPad wall panel (Guided Access / Single App Mode).
- Matter bridge: expose VESTA devices to Apple Home and Google Home.
- Optional Home Assistant bridge driver; MQTT / Zigbee2MQTT driver.
- APNs relay for iOS push.

## Challenges & Risks

| Challenge | Why it is hard | Mitigation |
|---|---|---|
| Configuration from the UI (no JSON editing) | Largest chunk of work; decides whether VESTA is really out-of-the-box | Start with a simple editor; JSON is only the persistence format |
| Android specifics | Device Owner needed for real kiosk and silent OTA; `go2rtc` can't be a Tauri sidecar on Android and can't run from writable storage (ship as `lib*.so` in `jniLibs`); Hub must live in a Foreground Service | Early spike on real hardware |
| Pairing & secure tunnel | Must be right without inventing crypto; local proxy on every platform | iroh (QUIC/TLS 1.3), design review before implementation |
| Device APIs without a password (e.g. EV chargers on the home Wi-Fi) | Anyone on the network can already control them; their safety settings must never be touched by mistake | VESTA adds no exposure; drivers write only a closed list of everyday settings, tested (AGENTS.md rule 7) |
| Tuya onboarding | `local_key` requires a one-time Tuya cloud account; several protocol versions (3.3/3.4/3.5); data points need per-category mapping | Acceptable for v1; not the showcase driver for new users |
| Remote access behind CGNAT | Direct connection may fail; free relays are best-effort | Relay as plan B; WebRTC peer-to-peer for media |
| Doorbell hardware | Few models offer two-way audio that `go2rtc` supports natively | Pick a model only after a spike |
| iPad as a wall panel | Apps can't turn the screen off; kiosk relies on Guided Access | Dim + black overlay; wake on device events |
| iOS push | APNs keys can't be distributed in an open-source repo | ntfy first; relay later |
| Solar inverters without a local API | Some inverters only expose data through the vendor cloud | Dedicated power meter instead |
| Homes with several routers | Extra routers left in router mode create separate networks (double NAT): the hub cannot discover or reach devices behind them, and phones on that network cannot find the hub | Setup runs a network check (several gateways or subnets, devices that never answer) and explains the fix in plain words, e.g. "put this router in bridge mode" |

## Open Questions
- **Doorbell recommendation** — mid-range, PoE, `go2rtc`-native two-way audio. Reolink doorbells are ruled out for now: talkback is not reliably available over RTSP/ONVIF.
- **Inverters without a local API** — e.g. Huawei SUN2000 without an SDongle: Modbus TCP on the home LAN has been closed since late-2021 firmware.
- **Tuya onboarding** without requiring a developer account: the plan is the Smart Life / Tuya Smart QR login (user code + QR, Tuya's device-sharing SDK — what Home Assistant has used since 2024.2) to fetch the local keys **once**, then local control only. Needs VESTA's own Tuya client registration (never reuse another project's). Until then: `tinytuya wizard` + `vesta-hubd tuya-import`.

## Costs for Maintainers
End users pay nothing. Maintainers should expect:
- Apple Developer Program (yearly fee) to distribute the iOS/iPadOS app.
- Optionally, a domain for pairing universal links (a GitHub Pages subdomain also works).
- Google's developer verification requirements for Android apps distributed outside Play — to be reviewed before the first Android release.
