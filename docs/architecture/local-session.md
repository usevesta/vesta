# Local session

How the panel proves to its **local endpoint** that it is the panel. The local endpoint is the
Hub on the wall tablet (and on a desktop running the app with `--features hub`), and later the
`vesta-link` proxy on phones. Code: `crates/vesta-hub/src/auth.rs` (sessions),
`crates/vesta-hub/src/assets.rs` (UI serving), `apps/tauri/src-tauri/src/{hub,panel}.rs` (the
shell).

## Why loopback is not enough

The endpoint only listens on `127.0.0.1`, but loopback is shared by **every program on the
device**: any other app on the Android tablet, any process on a desktop, and any web page open
in a browser on that machine (it can `fetch` and open WebSockets to `127.0.0.1`). The API opens
doors, so "the request came from this machine" proves nothing.

## The mechanism

1. **One origin.** The endpoint serves the UI and the API on the same origin
   (`http://127.0.0.1:<port>`), so the frontend uses same-origin relative URLs and is identical
   on every target.
2. **One-time login code.** The embedding shell asks the Hub for a code: 128 bits from the OS
   CSPRNG, base64url, single use, valid for 60 s. At most 8 are pending at once.
3. **Redeem.** The shell points the panel window at `GET /api/v1/session?code=…`. The Hub
   compares the code in constant time against the pending ones, removes it, creates a 256-bit
   session and answers `303 → /` with
   `Set-Cookie: vesta_session=…; HttpOnly; SameSite=Strict; Path=/` (no `Secure`: plain
   loopback HTTP), `Cache-Control: no-store` and `Referrer-Policy: no-referrer`. Failures are a
   bare `401`.
4. **Every API route needs the session**, the WebSocket included, except `/api/v1/health` and
   the redeem route. Without a valid cookie: `401`, no details.
5. **Exact origins.** Cookies ignore ports (RFC 6265 §8.5) and `SameSite` treats
   `127.0.0.1:3000` and `127.0.0.1:8080` as the same site, so a cookie alone would let a page
   served by *another* local server use the session. A session is therefore only honoured when
   `Host` and, if present, `Origin` are exactly one of the allowed origins: the Hub's own
   (`http://127.0.0.1:<port>`, `http://localhost:<port>`) plus origins configured explicitly
   (`HubOptions::allowed_origins`, e.g. the Vite dev server). Anything else is `403`.
   Browsers always send `Origin` on cross-origin requests and on WebSocket handshakes.
6. **Identity.** The auth layer is the single place that decides who is calling: it inserts an
   `Identity { kind, role }` request extension (`LocalPanel`/`Admin` today). Handlers never
   read cookies. `vesta-link` will add identities for paired phones (Admin, Member, Guest).

Sessions live in memory (at most 32; the oldest is dropped). A restart signs everyone out and
the shell signs the panel in again on launch. No secret ever reaches JavaScript: the cookie is
`HttpOnly` and the code is spent by the time the page runs.

## Who uses it

| Runner | Sign-in |
|---|---|
| The app, release (`--features hub`) | Binds `127.0.0.1` (last run's port if free, so the origin and per-screen settings survive restarts; any free port otherwise), serves the embedded `web/dist`, opens the window at `http://127.0.0.1:<port>/api/v1/session?code=…`. |
| The app, `pnpm tauri dev` | The Hub listens where Vite proxies `/api` (`127.0.0.1:8181`, or `VESTA_HUB_ORIGIN`) and allows exactly the dev server's origin (`http://localhost:1420`); the window opens `http://localhost:1420/api/v1/session?code=…` through the proxy, with hot reload. |
| `vesta-hubd` | Logs a one-time sign-in URL at start-up (like Jupyter). `--ui web/dist` serves the UI on the hub's own origin; `--allow-origin http://localhost:5173` lets the Vite dev UI use a session. |
| `vesta-hubd --dev` | `ApiAuth::Disabled` (below). |

## The app shell

- The window may only navigate within the origin it opened; new windows are refused.
- Its page gets Tauri IPC for the kiosk commands (`kiosk_*`) and nothing else. For the Hub's
  page, which Tauri treats as a remote origin, that capability is added at runtime for the
  **exact** origin the shell just bound (`http://127.0.0.1:<port>`), never a pattern such as
  `http://127.0.0.1:*`.
- The UI's `Content-Security-Policy`: `default-src 'self'; script-src 'self'; style-src 'self'
  'unsafe-inline'` (UI libraries insert `<style>` elements); `connect-src 'self'` plus, in the
  app, Tauri's IPC transport (`ipc:`, `http://ipc.localhost`); `object-src 'none'; base-uri
  'none'; form-action 'none'; frame-ancestors 'none'`. Plus `X-Content-Type-Options: nosniff`
  and `Referrer-Policy: no-referrer`. Tauri's own CSP for `tauri://` is off: the Hub serves the
  page and sends the policy.

## `ApiAuth::Disabled`

No authentication: every request that passes the local-only guard (local `Host`, local
`Origin`) acts as an admin. It exists only for the development loop (`vesta-hubd --dev`, the
end-to-end tests) and is never the default; the Hub logs a warning when it is on. On a machine
running a `--dev` hub, any program and any local web page can control the simulated home.

## Threats

| Threat | Outcome |
|---|---|
| Another app or process on the device calls the API | No cookie: `401`. Only `/api/v1/health` answers. |
| A web page from the internet, in a browser on the same machine | Cross-site: the `SameSite=Strict` cookie is not sent; the local-only guard also refuses foreign `Origin`s. |
| A page served by another local server (`http://127.0.0.1:3000`) | Same site for cookies, but not an allowed origin: `403`. |
| DNS rebinding (`attacker.example` resolving to `127.0.0.1`) | Non-local `Host`: `403` (local-only guard). |
| Guessing a code or session | 128/256 bits from the OS CSPRNG, constant-time compare, codes single use and short-lived. |
| The code leaking from the URL | Single use and spent before the page runs; `Referrer-Policy: no-referrer`; `Debug` never prints it. `vesta-hubd` logs it on purpose, for whoever can read its logs. |
| Script injection in the panel | `script-src 'self'`; the cookie is `HttpOnly`. |
| Port squatting by a local app | The shell only ever opens the port it bound itself; a taken port just means another free one. |
| A desktop browser sending the cookie to other local servers it visits on the same host | Possible with `vesta-hubd` in a desktop browser (cookies ignore ports). Not in the app: its webview has its own cookie store and cannot leave the panel's origin. |
| Malware with the user's privileges reading the app's memory or webview storage | Out of scope: it already controls the device. |
