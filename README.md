# VESTA

[![CI](https://github.com/usevesta/vesta/actions/workflows/ci.yml/badge.svg)](https://github.com/usevesta/vesta/actions/workflows/ci.yml)

Local-first smart home wall panel built with Tauri v2, Rust and React. An appliance, not a
platform: install it, scan a QR code, hang it on the wall.

- **Everything that controls the home runs locally**, on the panel itself (an Android tablet)
  or on a small headless hub (`vesta-hubd`). No cloud account is needed to switch a light.
- **Brand-free by design**: devices are described by capabilities (on/off, level, cover, fan,
  ...) modeled after Matter; each protocol lives in its own driver crate.
- **The UI is data**: pages and widgets come from a dashboard the home serves, rendered through
  a widget registry and themed with CSS variables only.
- **If VESTA fails, the home keeps working** like a normal home.

Architecture notes and the roadmap live in [`docs/`](docs/); contributor rules are in
[`AGENTS.md`](AGENTS.md).

## Development loop

No tablet or hardware needed: the hub can run a simulated home.

```sh
# Terminal 1: the hub with a simulated home on http://127.0.0.1:8080 (sign-in off)
cargo run -p vesta-hubd -- --dev

# Terminal 2: the wall panel UI
cd web
pnpm install
pnpm dev
```

Open the `http://localhost:5173/` URL Vite prints. The dev server proxies `/api` (HTTP and the
WebSocket) to the hub. If the hub runs on another port (`--port 9000`), start the UI with
`VESTA_HUB_ORIGIN=http://127.0.0.1:9000 pnpm dev`.

The simulated home has lights, a ceiling fan with a light, a blind, a garage door, a door strike
and a doorbell that rings every 3 minutes (to see the doorbell screen).

Tap **Edit** on the screen to add, move, resize, rename or remove tiles and to manage pages. With
`--dev`, changes live in memory only (restarting the hub resets the simulated home); with
`vesta-hubd --config config.json --dashboard dashboard.json` they are saved back to that file.

To look at the UI without a hub, open `http://localhost:5173/?demo=1` (a sample home with mock
state). Add `&lang=es` to force Spanish.

Without `--dev` the hub needs a sign-in: it logs a one-time URL at start-up (single use, valid
for 60 s) to open in a browser on the same machine. Add `--ui web/dist` (after `pnpm build`) to
serve the panel from the hub itself, or `--allow-origin http://localhost:5173` to use the Vite
dev UI (open the same `/api/v1/session?code=…` path on `http://localhost:5173`). See
[`docs/architecture/local-session.md`](docs/architecture/local-session.md).

### The app

`apps/tauri` is the Tauri v2 app. With `--features hub` it runs the hub in-process and shows the
wall panel in a native window. Without a home set up (`config.json` + `dashboard.json` in the
app data folder, e.g. `~/Library/Application Support/com.usevesta.app/` on macOS) it runs the
simulated home.

```sh
cd web && pnpm install && cd ../apps/tauri && pnpm install

# Hot reload: Vite on http://localhost:1420, the in-process hub on 127.0.0.1:8181,
# so it can run next to the browser loop above.
pnpm tauri dev --features hub

# A debug build of the macOS app (target/debug/bundle/macos/VESTA.app)
pnpm tauri build --features hub --debug --bundles app
```

Android and iOS are prepared (platform code behind `cfg`, native kiosk plugins still to come)
but not built yet. iOS/iPadOS never get the `hub` feature.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

cd web
pnpm typecheck && pnpm lint && pnpm test && pnpm build
pnpm test:e2e   # end-to-end tests (live control, editing the screen) against a real vesta-hubd --dev

cd ../apps/tauri
pnpm tauri build --features hub --debug --bundles app   # the app still builds
```

`cargo test` also regenerates the TypeScript bindings in `web/src/api/generated/` (the app's
kiosk command types in its `shell/` folder). The workspace includes the Tauri app, so building
it on Linux needs the WebKitGTK development packages
([Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)).

`pnpm test:e2e` starts its own `vesta-hubd --dev` with `cargo run` on a free port and stops it
afterwards. To use a hub that is already running instead:
`VESTA_HUB_URL=http://127.0.0.1:8080 pnpm test:e2e`.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
