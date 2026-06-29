# Milestone 20 — Docker Compose topologies (single & distributed)

## Goal

Ship two ready-to-run Docker Compose topologies so the app can be launched as a
container, and so **we** can exercise the distributed-systems behaviour
(leader-elected controller, per-repo/per-job locks, shared platform model)
locally:

1. **Single instance** — one app container on **SQLite**, no external
   dependencies. The zero-config "just run it" deployment.
2. **Distributed** — **two** app instances behind an **nginx** load balancer,
   sharing **PostgreSQL** + **Redis**. Used to validate that two instances
   coordinate correctly (only one controller leader, locks serialise work across
   instances, both serve the same model).

A production-style multi-stage `Dockerfile` underpins both.

## Scope

- A multi-stage `Dockerfile` building a small runtime image of the app.
- `docker-compose.single.yml`: one app container + SQLite (named volume for the
  DB file and the workspace), no other services.
- `docker-compose.distributed.yml`: `app1` + `app2` + `nginx` + `postgres` +
  `redis`, plus the existing dbmate migrate step for Postgres.
- An nginx config that load-balances the two app instances.
- `config.yaml` files (per topology) wiring engine/Redis via M18.
- `bin/up.*` / `bin/down.*` select a topology (argument), per the project's
  compose-script convention.

## Deliverables

### Dockerfile

A multi-stage build (no source in the final image):

- **Builder**: `rust:1.85` (matches `rust-version`), `cargo build --release`,
  with dependency-layer caching.
- **Runtime**: a slim base (e.g. `debian:bookworm-slim`), copy the release
  binary, the `assets/` and `db/` directories (embedded migrations already cover
  SQLite; `db/` is mounted for dbmate in the distributed topology), a non-root
  user, `EXPOSE 8080`, entrypoint `platform-inspector`.
- The image reads `config.yaml` (M18) from a mounted path or `--config-file`;
  secrets stay in the environment and are referenced via `${VAR}`.

### Single-instance topology

`docker-compose.single.yml`:

```yaml
services:
  app:
    build: .
    ports: ["8080:8080"]
    volumes:
      - app_data:/data                       # sqlite db + workspace persist here
      - ./deploy/single/config.yaml:/app/config.yaml:ro
    environment:
      ENCRYPTION_KEY: "${ENCRYPTION_KEY}"     # required; no insecure default in containers
    command: ["--config-file", "/app/config.yaml"]
volumes:
  app_data:
```

`deploy/single/config.yaml` sets `database.url: sqlite:///data/platform_inspector.db?mode=rwc`,
`workspace_dir: /data/workspace`, `redis.enabled: false`.

### Distributed topology

`docker-compose.distributed.yml` — two app instances sharing Postgres + Redis,
fronted by nginx:

```yaml
services:
  db:        # postgres:16-alpine (reuse the existing service + healthcheck)
  redis:     # redis:7-alpine, healthcheck redis-cli ping
  dbmate:    # amacneil/dbmate:2.28.0, profile-gated migrate (as today)
  app1: &app
    build: .
    volumes: ["./deploy/distributed/config.yaml:/app/config.yaml:ro"]
    environment:
      ENCRYPTION_KEY: "${ENCRYPTION_KEY}"
      SESSION_SECRET: "${SESSION_SECRET}"     # shared so sessions validate on either instance
    command: ["--config-file", "/app/config.yaml"]
    depends_on:
      db: { condition: service_healthy }
      redis: { condition: service_healthy }
  app2: *app
  nginx:
    image: nginx:1.27-alpine
    ports: ["8080:80"]
    volumes: ["./deploy/distributed/nginx.conf:/etc/nginx/nginx.conf:ro"]
    depends_on: [app1, app2]
```

`deploy/distributed/config.yaml`: `database.url: ${DATABASE_URL}` (Postgres),
`redis.enabled: true`, `redis.url: redis://redis:6379`, `workspace_dir` on a
shared volume. The two app instances **must** share `ENCRYPTION_KEY` (decrypt the
same secrets) and `SESSION_SECRET` (a session is valid on either instance).

`deploy/distributed/nginx.conf` — upstream over the two instances:

```nginx
events {}
http {
  upstream app { server app1:8080; server app2:8080; }
  server {
    listen 80;
    location / { proxy_pass http://app; proxy_set_header Host $host; }
  }
}
```

### bin scripts (topology argument)

Extend the existing helpers (which already accept an optional argument) so the
first argument selects the topology/compose file, keeping the project's
`bin/up.*` + `bin/down.*` convention:

- `bin/up.sh [single|distributed] [profile]` → runs `down` first, then
  `docker compose -f docker-compose.<topology>.yml up` (default `single`);
  `distributed` brings up the migrate profile first, then the apps + nginx.
- `bin/down.sh [single|distributed]` → `docker compose -f … rm -f --all`.
- Mirror in `bin/up.bat` / `bin/down.bat`.

## Tasks

- [ ] Multi-stage `Dockerfile` (builder + slim runtime, non-root, assets + db).
- [ ] `docker-compose.single.yml` + `deploy/single/config.yaml`.
- [ ] `docker-compose.distributed.yml` (app1/app2/nginx/postgres/redis) +
      `deploy/distributed/config.yaml` + `nginx.conf`.
- [ ] Redis service + healthcheck; reuse the existing Postgres + dbmate services.
- [ ] `bin/up.*` / `bin/down.*` topology argument.
- [ ] README "Running with Docker" section documenting both topologies, required
      `ENCRYPTION_KEY`/`SESSION_SECRET`, and the migrate step for Postgres.

## Acceptance criteria

- `bin/up.sh single` brings up one container on SQLite with no other services and
  serves the app on `:8080`; data persists across restarts via the volume.
- `bin/up.sh distributed` brings up two app instances + nginx + Postgres + Redis;
  requests are balanced across both instances and both render the same platform
  model from the shared database.
- With two instances running, exactly one holds the controller leader lease and
  jobs/locks serialise across instances (via the M19 Redis lock) — no double
  execution.
- Both topologies start from a clean checkout with only `ENCRYPTION_KEY` (and, for
  distributed, `SESSION_SECRET`) provided.

## Dependencies

Milestones 18 (`config.yaml` + `--config-file`), 19 (Redis lock for cross-instance
coordination), 12/13 (controller leader election + locks), and the existing
Postgres/dbmate compose services.

## Out of scope

Production hardening (TLS termination, secrets managers, orchestration/Helm),
autoscaling beyond the fixed two instances, and CI wiring of the distributed
stack — these compose files are for local/manual testing.
