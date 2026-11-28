# VESTA (`usevesta/vesta`)

## Project Overview
Open-source (Apache-2.0), local-first smart home wall panel built with **Tauri v2**, **Rust**, and **React + `shadcn/ui`**. VESTA aims to be an **appliance, not a platform**: install it, scan a QR code, hang it on the wall.

- **No cloud dependency for home control:** Everything that controls the home runs on the local Hub. Optional external services (push delivery, update downloads, relay fallback for remote access) must never be required for local operation.
- **Hub topologies:**
  - **Wall tablet (primary):** an Android tablet is both the kiosk UI and the Hub (`--features hub`).
  - **Headless hub:** `vesta-hubd` runs on a Linux box with no screen; an iPad (or any client) acts as the wall panel. During development it runs on macOS and the UI runs in a regular browser.
- **Thin clients:** iOS, iPadOS, Android, macOS, Windows, and Linux clients share the exact same codebase (compiled without `hub`). One client can be paired with **multiple homes**.
- **Extensibility first:** Hardware is abstracted by *capabilities*, UI is driven by a declarative `dashboard.json`, and themes use pure CSS variables.

## Target Users
- **Primary persona:** a non-technical household member — e.g. a tradesperson who is comfortable with wiring but gets lost as soon as anyone mentions servers, links, IPs or apps. If they would not understand a word, it must not appear in the UI.
- **Installer:** someone who sets VESTA up for a household and hands it over.
- **Power users** (e.g. an iPad wall panel + headless hub) are welcome, but never at the expense of the primary persona.

## Monorepo Structure
- `crates/vesta-core`: Domain model (Device → Endpoint → Capability), capabilities as plain serializable data types (not trait objects), the single `DeviceDriver` trait, `EventBus`, and versioned config/dashboard schemas. Spec: `docs/architecture/capabilities.md`.
- `crates/vesta-hub`: Library with the Axum HTTP/WebSocket API, device runtime, and `go2rtc` lifecycle manager. Only compiled with `--features hub`.
- `crates/vesta-link`: Pairing (QR code + per-home device keys), the encrypted iroh tunnel, and the client-side local proxy.
- `crates/vesta-drivers`: Composition root that registers driver factories (one Cargo feature per driver). The **only** place that maps a config `driver` string to a driver crate.
- `crates/drivers/*`: One isolated crate per protocol/vendor (e.g. `vesta-driver-tuya`, `vesta-driver-shelly`, `vesta-driver-onvif`). Drivers MUST NOT depend on each other or on UI/server crates. Drivers are hub-only.
- `apps/hubd`: Headless hub binary (`vesta-hubd`) without Tauri/WebView. Used for Linux deployments and as the fast development loop.
- `apps/tauri`: Tauri v2 shell (`com.usevesta.app`) and Cargo feature gates. Native code lives in Tauri plugins (`apps/tauri/plugins/*`, e.g. `tauri-plugin-vesta-panel`), not in hand-edited generated projects (`src-tauri/gen/*` is committed CLI output):
  - Android (Kotlin): Foreground Service hosting the Hub, WakeLock, LockTask kiosk (Device Owner), silent OTA via `PackageInstaller`.
  - iOS/iPadOS (Swift): idle timer, screen brightness, Guided Access detection.
- `web`: React + TypeScript + Vite + Tailwind + `shadcn/ui`. No other component libraries.
- `docs/`: Roadmap and architecture notes.

## Core Architectural Rules (Non-Negotiable)
1. **Capability-Based Abstraction:** The frontend and `vesta-hub` MUST NEVER know about hardware brands (e.g. no `if (device.brand === 'shelly')`). Everything maps to standard capabilities defined in `vesta-core`, modeled after Matter's data model so a future Matter bridge maps ~1:1. Capabilities are **reported by drivers**; user config may only override presentation (name, room, endpoint type).
2. **Unified Client/Hub Communication:** The React frontend always talks HTTP/WebSocket to a **local** endpoint (`http://127.0.0.1:<port>`).
   - On the Hub device, that endpoint is the Hub itself, protected by a per-session token injected by the Tauri shell (any app on the device can reach `localhost`).
   - On client devices, it is a local proxy in the Rust layer (`vesta-link`) that tunnels to the paired Hub over an authenticated, encrypted iroh connection.
   - In development, the browser talks to `vesta-hubd` directly.
   - Frontend logic is 100% identical across targets. The Hub never exposes plain HTTP on the LAN.
3. **No Native RTSP in WebView:** All camera/doorbell streams are converted to WebRTC (with two-way audio) by the embedded `go2rtc` binary on the Hub. The frontend only consumes standard WebRTC `<video>` streams. The `go2rtc` API binds to `127.0.0.1` only; the Hub proxies WebRTC signaling. Media flows peer-to-peer over WebRTC; if no direct path exists, fall back to a lower-quality stream through the tunnel.
4. **Platform Boundaries:** Isolate OS-specific code behind `#[cfg(target_os = "...")]`. iOS/iPadOS are strictly **Client-only** (never compile `hub` or bundle `go2rtc` for iOS). Kiosk behavior goes through a platform-neutral `KioskHost` abstraction (Android, iOS/iPadOS, desktop).
5. **Declarative UI & Widget Registry:** Never hardcode rooms or devices in React. All views are rendered from `dashboard.json` via `web/src/widgets/registry.ts`. **Users never edit JSON:** everything is configurable from the UI; JSON files are only the persistence format and carry a `schema_version`.
6. **Themeable by Default:** Never hardcode hex/Tailwind palette colors in components (e.g. no `bg-slate-900`). Always use `shadcn/ui` semantic CSS variables (`bg-card`, `text-foreground`, `border-border`) so themes can be swapped dynamically.
7. **Safe Degradation:** If VESTA (hub, tablet, network or a driver) fails, the home keeps working like a normal home: wall switches still switch lights, the doorbell still rings, doors open with a key. VESTA adds features; it is never the only way to do something essential. Never take over safety functions a device already handles (e.g. an EV charger's contracted-power limit).
8. **Secure by Default:**
   - Pairing uses a QR code shown on the Hub's own screen (physical presence = authorization), containing the Hub's public key and a one-time 128-bit secret with a short expiry. A numeric-code fallback must use a PAKE (e.g. SPAKE2).
   - Each client generates a separate key pair **per home**, stored in the OS keystore. Private keys are never transmitted.
   - Every connection is mutually authenticated. Roles: Admin, Member, Guest (optionally time-limited). Any paired device can be revoked from the Hub UI.
   - Cryptography lives in Rust, never in the WebView. Never invent crypto: use iroh (QUIC/TLS 1.3) and standard primitives.
   - The API never exposes device secrets, driver names or driver config.
   - Sensitive commands (unlock, open door/gate/garage) require an authorized role and biometric confirmation on mobile.
9. **Plain-Language UI:** User-facing text follows this glossary. All UI strings are translatable (at least `en` and `es`).

   | Internal term | UI (en) | UI (es) |
   |---|---|---|
   | Hub | "your home" / "the wall panel" | "tu casa" / "la pantalla" |
   | Pair a client | "Add a phone" | "Añadir un móvil" |
   | Revoke a client | "Remove" | "Quitar" |
   | Endpoint | its name, e.g. "Living room light" | su nombre, p. ej. "Luz del salón" |
   | Driver, capability, endpoint, IP, port, server, token, relay, tunnel | never shown | nunca se muestra |

## Connectivity
- **At home:** direct LAN connection through iroh (local discovery). No cloud round-trip.
- **Away:** iroh NAT traversal; relay fallback (n0 public relays by default, self-hosted relay configurable). Relays only see encrypted traffic. Design so the relay is plan B, not the normal path.
- **No Tailscale, no port forwarding, no third-party accounts** for end users.
- **Push notifications:** ntfy first (self-hostable). An APNs relay is a later concern (APNs keys can never ship in this repo).

## Coding Standards

### Rust
- Strictly no `.unwrap()` or `.expect()` in production paths; propagate errors with `thiserror` (in `vesta-core` / drivers) or `anyhow` (in `vesta-hub` / apps).
- Use `tokio` for async I/O and `tokio::sync::broadcast` for the internal `EventBus`. Treat `RecvError::Lagged` as "resync": send the client a full state snapshot.
- Every new driver in `crates/drivers/` must include unit tests with mocked HTTP/TCP payloads.
- Keep `hub` dependencies strictly behind `#[cfg(feature = "hub")]` so mobile client builds stay lightweight.
- API types are defined once in Rust and exported to TypeScript (e.g. `ts-rs` / `specta`). Never hand-write API types in the frontend.

### Frontend (React + TypeScript)
- React + `shadcn/ui` only; do not add other UI component libraries.
- Icons: Lucide (`lucide-react`) only, rendered through the semantic icon registry (`<Icon name="fan" />`), never imported ad hoc in components. Never add Font Awesome Pro or other non-redistributable assets — this is a public repo.
- Strict TypeScript (`noImplicitAny`, no `any`).
- State management via Zustand synced over a single WebSocket connection with automatic exponential-backoff reconnection (critical for Wi-Fi <-> 5G handoffs). On reconnect, request a full snapshot.
- **Wall-first ergonomics:** Primary interactive controls must have large touch targets (`min-h-14` / `min-h-16`) and high contrast in dark mode.
- Commands are optimistic: show a pending state until the device confirms; revert and show an error on timeout.

### Security & Privacy
- Never commit real `config.json`, `dashboard.json`, device IPs, Tuya `local_key`s, hub/client identity keys, APNs `.p8` keys, or Android keystores. Only maintain `*.example.json`.
- **This is a public repository.** Never include personal data in code, docs, examples, tests, fixtures or commit messages: real names or family references, home addresses or cities, device serial numbers, real IPs/MACs, Wi-Fi names. Use generic personas and placeholders.

## Common Commands
- **Dev loop (no tablet needed):** `cargo run -p vesta-hubd -- --dev` (simulated home, sign-in off) in one terminal, `cd web && pnpm dev` in another (Vite proxies `/api`, WebSocket included, to the hub).
- **Dev loop (the app, hot reload):** `cd apps/tauri && pnpm tauri dev --features hub` (in-process hub on `127.0.0.1:8181`, Vite on `localhost:1420`).
- **Build the desktop app (macOS):** `cd apps/tauri && pnpm tauri build --features hub --debug --bundles app`
- **Check Rust workspace:** `cargo check --workspace --all-features`
- **Lint & Format Rust:** `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings`
- **Run Rust tests:** `cargo test --workspace --all-features`
- **Frontend typecheck & lint:** `cd web && pnpm typecheck && pnpm lint`
- **Build Android Hub (Tablet), debug APK:** `cd apps/tauri && pnpm tauri android build --debug --target aarch64 --apk --features hub` (needs `ANDROID_HOME`, `NDK_HOME`, a JDK 17 in `JAVA_HOME`; see README)
- **Run it on a device or emulator:** `adb install -r apps/tauri/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk && adb shell am start -n com.usevesta.app/.MainActivity`, logs with `adb logcat -s RustStdoutStderr:V Tauri/Console:V`. `pnpm tauri android dev` does not work with `hub` (README explains why).
- **Lint the Android build:** `cargo clippy -p vesta-app --target aarch64-linux-android --features hub -- -D warnings`
- **Android native code:** `apps/tauri/plugins/panel` (Tauri plugin, Kotlin). `apps/tauri/src-tauri/gen/android` is CLI output: regenerate it (`pnpm tauri android init`), never hand-edit it.
- **Build Mobile Client (iOS, not set up yet):** `cd apps/tauri && pnpm tauri ios build` (without `hub`)

## PR & Contribution Guidelines
- Keep PRs focused: one driver, one widget, or one theme per PR.
- Before finishing any task, always run `cargo clippy`, `cargo test`, and `pnpm typecheck` and ensure zero warnings.
