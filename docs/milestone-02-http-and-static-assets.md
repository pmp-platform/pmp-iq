# Milestone 02 — HTTP API foundation & local static assets

## Goal

Turn the skeleton into a real web app shell: a routed HTTP API, server-rendered
HTML layout, and **locally served** jQuery + Tailwind (no CDNs). Establish the
navigation chrome (Settings / Jobs / Platform) that later milestones fill in.

## Scope

- `axum` router with API and page routes, error handling, and JSON envelope.
- HTML templating with a shared base layout.
- Download and vendor jQuery + Tailwind CSS into `assets/`, served from disk.
- App shell: top navigation, empty Settings / Jobs / Platform pages.

## Deliverables

### Routing & API conventions

- Split routes into modules (`routes/api`, `routes/pages`) and merge in the app
  builder.
- Standard JSON response envelope and a single error type that maps to HTTP
  status codes (`AppError` → `IntoResponse`).
- Request tracing/logging middleware.

### Templating

- Server-side templates (e.g. `askama` or `minijinja`) with a `base.html`
  layout: `<head>` linking local assets, a nav bar, and a content block.
- A `Page` helper to render templates with common context (current user, active
  nav item) — kept under the parameter limit via a context struct.

### Local assets (must be served locally)

- Download into `assets/vendor/`:
  - jQuery (`jquery.min.js`).
  - Tailwind CSS. Prefer the **Tailwind CLI** build producing a single
    `assets/vendor/tailwind.css` from the project's templates; the standalone
    build output is committed so runtime needs no Node/CDN.
- Serve `assets/` via a static file route (`/assets/*`).
- Document the asset refresh procedure in `README.md` (how to re-download jQuery
  and rebuild Tailwind).

### App shell

- Nav bar with links: **Settings**, **Jobs**, **Platform**.
- Each section renders a placeholder page extending `base.html`.

## Tasks

- [ ] Add templating + `tower-http` (static files, trace, compression).
- [ ] Implement `AppError` and the JSON envelope.
- [ ] Build `base.html` + per-section placeholder templates.
- [ ] Download jQuery into `assets/vendor/`; set up Tailwind CLI build to
      `assets/vendor/tailwind.css`.
- [ ] Add the static-file route and verify assets load with no external requests.
- [ ] Add a tiny jQuery interaction to prove wiring (e.g. nav active state / a
      ping to `/healthz`).

## Acceptance criteria

- Visiting `/` shows the shell with working nav to Settings/Jobs/Platform pages.
- Browser dev tools show **all** JS/CSS loaded from `/assets/...` — zero external
  network calls.
- API errors return the standard JSON envelope with correct status codes.
- Route handlers are thin; rendering/util logic lives in helpers with unit tests.

## Dependencies

Milestones 00–01.

## Out of scope

Authentication (next milestone), real section content.
