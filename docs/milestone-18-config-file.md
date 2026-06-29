# Milestone 18 — Optional `config.yaml` configuration file

## Goal

Add an **optional YAML configuration file** as a second source of configuration
alongside the existing environment variables. Today everything is loaded from
the process environment via `Config::load(&dyn EnvSource)` (`src/config.rs`).
This milestone layers a `config.yaml` on top so an operator can keep all settings
(database engine + credentials, Redis, log level, auth provider, server, …) in
one file, while still being able to **pull individual values from environment
variables** via `${VAR}` interpolation.

By default the app looks for `config.yaml` next to the running binary; the path
is overridable with a `--config-file` flag. The file is **optional** — with no
file present, behaviour is identical to today (pure env + built-in defaults).

## Scope

- Discover an optional config file (binary-adjacent by default, `--config-file`
  override) and parse it into the existing `Config` shape.
- `${VAR}` / `${VAR:-default}` interpolation of environment variables inside file
  values, so secrets stay in the environment but structure lives in the file.
- A clear precedence order: explicit process env var > file value > built-in
  default — preserving today's 12-factor overrides.
- New config sections consumed by later milestones: `redis` (disabled by
  default, M19) and `auth.provider` (`admin` default, `github` opt-in, M21);
  plus `log.level`.
- All file access behind the existing `FileSystem` trait so config loading stays
  unit-testable with no real filesystem.

## Deliverables

### CLI flag + file discovery

`main.rs` parses arguments and resolves the config path (entrypoint-only —
no business logic):

- Add a tiny `Cli` parser (`clap` derive, or a hand-rolled `--config-file <path>`
  reader) exposing `config_file: Option<String>`.
- Resolution order for the path:
  1. `--config-file <path>` when given (error if that explicit path is missing).
  2. else `config.yaml` next to the executable (`current_exe()` parent).
  3. else current working directory `./config.yaml`.
- A missing file at a **defaulted** location is not an error (file is optional);
  a missing file at an **explicit** `--config-file` path is a hard error.

### Layered loader

Introduce a `ConfigLoader` that composes the file and the environment behind
traits (so it is fully mockable):

```rust
pub struct ConfigLoader<'a> {
    pub fs: &'a dyn FileSystem,   // src/fs.rs — read_to_string / exists
    pub env: &'a dyn EnvSource,   // src/config.rs — process env
}

impl ConfigLoader<'_> {
    /// Read the optional file, interpolate ${VAR} refs from `env`, then build
    /// `Config` with precedence: env var > file value > default.
    pub fn load(&self, path: Option<&str>) -> Result<Config, ConfigError>;
}
```

- The raw YAML deserialises into a `FileConfig` mirror struct (all fields
  `Option`), kept separate from the runtime `Config` so missing keys fall through
  to env/defaults.
- `Config::load(&dyn EnvSource)` stays as the env-only path (back-compat +
  existing tests); the layered loader builds on the same field-by-field
  resolution helpers (extend `parse_u16`/`parse_u32`/… to take an optional file
  value as the fallback before the default).

### Environment-variable interpolation

A reusable helper in `src/strings.rs` (DRY — string utilities live there):

```rust
/// Replace ${VAR} and ${VAR:-default} in `raw` using `env`. An unset variable
/// with no default resolves to empty; a literal `$$` escapes to `$`.
pub fn interpolate_env(raw: &str, env: &dyn EnvSource) -> String;
```

Applied to every string value read from the file before it is used, so the file
can reference secrets that remain in the environment:

```yaml
database:
  url: "${DATABASE_URL:-sqlite://platform_inspector.db?mode=rwc}"
auth:
  github:
    client_secret: "${GITHUB_CLIENT_SECRET}"
```

### File schema

The file maps onto the existing `Config` plus the new sections. Every field is
optional; shown with its effective default:

```yaml
server:
  bind_address: 0.0.0.0
  port: 8080
  assets_dir: assets

database:
  # engine is inferred from the url scheme (postgres:// → Postgres, else SQLite)
  url: "sqlite://platform_inspector.db?mode=rwc"
  max_connections: 10

redis:                       # consumed in M19; disabled by default
  enabled: false
  url: "redis://localhost:6379"

auth:
  provider: admin            # admin (default) | github  (github wired in M21)
  admin_user: "${ADMIN_USER}"
  admin_pass: "${ADMIN_PASS}"

log:
  level: info                # maps to the tracing EnvFilter (RUST_LOG still wins)

workspace_dir: tmp/workspace
git_min_interval_ms: 250
```

- Add `RedisConfig { enabled: bool, url: String }` and an `AuthProvider` enum
  (`Admin` | `Github`) to `Config`/`AuthConfig` now; M19 and M21 consume them.
- `log.level` feeds `telemetry::init` (build the `EnvFilter` from the config
  level, with `RUST_LOG` still taking precedence when set).

## Tasks

- [ ] `--config-file` flag + binary-adjacent discovery in `main.rs`.
- [ ] `FileConfig` mirror struct + YAML parse (`serde_yaml`/`serde_yml` dep).
- [ ] `ConfigLoader { fs, env }` with env > file > default precedence.
- [ ] `strings::interpolate_env` (`${VAR}` / `${VAR:-default}` / `$$`).
- [ ] `RedisConfig` + `AuthProvider` on `Config`; `log.level` → `telemetry::init`.
- [ ] Unit tests (mocked `FileSystem` + `MapEnv`): file-only load; env overrides
      file; `${VAR}` interpolation incl. default + unset; missing optional file is
      OK while missing explicit `--config-file` errors; invalid YAML reports a
      clear `ConfigError`.
- [ ] `config.example.yaml` at repo root + README config section.

## Acceptance criteria

- With no file present the app behaves exactly as today (env + defaults).
- A `config.yaml` next to the binary (or via `--config-file`) configures the
  database engine/credentials, Redis (off by default), log level, and auth
  provider; `${VAR}` references resolve from the environment.
- Process env vars still override file values; explicit `--config-file` that does
  not exist fails fast with a clear message.
- Config loading is unit-tested with a mocked `FileSystem` and in-memory env —
  no test reads a real file or the real process environment.

## Dependencies

Milestone 00 (config + `EnvSource`), and `src/fs.rs` `FileSystem`.

## Out of scope

Hot-reload / watching the file at runtime, TOML/JSON formats, and per-environment
profile merging (single file only). Redis usage (M19) and GitHub auth (M21)
consume the new sections but are specified in their own milestones.
