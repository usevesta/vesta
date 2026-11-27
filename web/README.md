# VESTA web

The wall panel UI: React + TypeScript + Vite + Tailwind CSS v4 + shadcn/ui.

```sh
pnpm install
pnpm dev          # http://localhost:5173, proxies /api (HTTP + WebSocket) to 127.0.0.1:8080
pnpm typecheck
pnpm lint
pnpm test
pnpm test:e2e     # against a real vesta-hubd --dev (started with cargo, or VESTA_HUB_URL=...)
pnpm build
```

Run the hub first (`cargo run -p vesta-hubd -- --dev` from the repo root), then `pnpm dev`.
`VESTA_HUB_ORIGIN` points the dev proxy at a hub on another port.

Open `http://localhost:5173/?demo=1` to see every widget with local mock state (no home
needed). Add `&lang=es` to force Spanish.

## Layout

| Path | What |
|---|---|
| `src/api/generated/` | TypeScript types generated from Rust. Never edit by hand. |
| `src/api/protocol.ts` | Runtime guards for everything the home sends (`ServerMessage`, dashboard). |
| `src/api/http.ts` | `GET /api/v1/dashboard` and `/devices`, decoded. |
| `src/live/` | The live app: the single connection (`live.ts`), device store with optimistic commands, widget bindings, screensaver and doorbell overlay. |
| `e2e/` | End-to-end smoke test against `vesta-hubd --dev` (`pnpm test:e2e`). |
| `src/widgets/registry.ts` | Widget type string → component. Unknown types show a placeholder. |
| `src/widgets/*/` | Presentational widgets with their own props (no wire types). |
| `src/components/dashboard/` | `DashboardGrid` (columns, spans, collapses on phones). |
| `src/components/shell/` | Full-screen shell, page navigation, display settings, connection banner. |
| `src/lib/ws/` | Reconnecting WebSocket client (backoff + jitter) and its Zustand store. |
| `src/lib/icons.ts` | Semantic icon registry. Render icons with `<Icon name="fan" />`. |
| `src/theme/` | Theme presets (`data-theme`), light/dark override, transparency toggle. |
| `src/i18n/` | `en` and `es` strings. Every user-facing string goes through `t()`. |
| `src/demo/` | Demo mode (`?demo=1`). |

## Rules of thumb

- Colors only through shadcn semantic tokens (`bg-card`, `text-foreground`…). Presets live in `src/index.css`.
- Icons only through `<Icon name="…" />`; add new names to `src/lib/icons.ts`.
- Plain language: never show words like server, IP, token or driver (a test enforces it).
- Big touch targets: primary controls are at least `min-h-14`.
