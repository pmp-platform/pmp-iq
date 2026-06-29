//! Application configuration.
//!
//! Configuration is layered: an optional `config.yaml` file (see
//! [`ConfigLoader`]) provides values, any of which can pull from the environment
//! via `${VAR}` interpolation; the process environment then overrides file
//! values, which override built-in defaults. Environment access is abstracted
//! behind [`EnvSource`] and file access behind [`FileSystem`], so loading is
//! fully unit-testable without touching the real environment or filesystem.

use crate::fs::FileSystem;
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

/// Abstraction over a source of environment variables.
///
/// Implemented by the real process environment ([`SystemEnv`]) and by an
/// in-memory map ([`MapEnv`]) used in tests.
pub trait EnvSource {
    fn get(&self, key: &str) -> Option<String>;
}

/// Reads variables from the real process environment.
pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// In-memory environment used for tests.
#[derive(Default, Clone)]
pub struct MapEnv {
    vars: HashMap<String, String>,
}

impl MapEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }
}

impl EnvSource for MapEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}

/// Error raised while loading configuration.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("invalid value for {key}: {reason}")]
    Invalid { key: String, reason: String },
}

fn invalid(key: &str, raw: &str, what: &str) -> ConfigError {
    ConfigError::Invalid {
        key: key.to_string(),
        reason: format!("'{raw}' is not a valid {what}"),
    }
}

/// Supported database engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbEngine {
    Postgres,
    Sqlite,
}

impl DbEngine {
    /// Infer the engine from a connection URL scheme.
    pub fn from_url(url: &str) -> Self {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            DbEngine::Postgres
        } else {
            DbEngine::Sqlite
        }
    }
}

/// Which login strategy the app authenticates with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthProvider {
    /// The built-in single admin account (default).
    Admin,
    /// GitHub authentication (M21).
    Github,
}

impl AuthProvider {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "github" => AuthProvider::Github,
            _ => AuthProvider::Admin,
        }
    }
}

/// How GitHub authentication is performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubAuthMode {
    /// OAuth authorization-code web flow ("Sign in with GitHub").
    OauthApp,
    /// The user presents a personal access token at the login form.
    PersonalToken,
}

impl GitHubAuthMode {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "personal_token" | "token" | "pat" => GitHubAuthMode::PersonalToken,
            _ => GitHubAuthMode::OauthApp,
        }
    }
}

/// GitHub authentication settings (used when `auth.provider` is `github`).
#[derive(Debug, Clone)]
pub struct GitHubAuthConfig {
    pub mode: GitHubAuthMode,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_url: Option<String>,
    /// API host (`https://api.github.com`, or a GHES API URL).
    pub api_base_url: String,
    /// Web host for the OAuth authorize/token endpoints (`https://github.com`).
    pub web_base_url: String,
    /// Allowlist: a user in any of these orgs may sign in.
    pub allowed_orgs: Vec<String>,
    /// Allowlist: a user with any of these logins may sign in.
    pub allowed_logins: Vec<String>,
}

/// Database connection settings. SQLite is the zero-config default;
/// PostgreSQL is used when the url points at a `postgres://` URL.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

impl DatabaseConfig {
    pub fn engine(&self) -> DbEngine {
        DbEngine::from_url(&self.url)
    }
}

/// HTTP server settings.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub assets_dir: String,
}

impl ServerConfig {
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind_address, self.port)
    }
}

/// Redis settings. Disabled by default; when enabled it backs the distributed
/// lock (M19) for multi-instance deployments.
#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub enabled: bool,
    pub url: String,
}

/// Authentication / session settings.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub provider: AuthProvider,
    pub admin_user: Option<String>,
    pub admin_pass: Option<String>,
    pub session_secret: String,
    /// Key used to encrypt secrets at rest (base64, 32 bytes once decoded).
    pub encryption_key: String,
    /// GitHub auth settings; present only when `provider` is `github`.
    pub github: Option<GitHubAuthConfig>,
}

/// Top-level application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub redis: RedisConfig,
    /// Logging filter/level fed to `telemetry::init` (`RUST_LOG` still wins).
    pub log_level: String,
    pub workspace_dir: String,
    /// Minimum interval (ms) between outbound git-provider API calls (throttle).
    pub git_min_interval_ms: u64,
    /// Cache-busting version appended to asset URLs (`?v=`). Taken from
    /// `APP_VERSION` when set, otherwise a random value generated per boot so
    /// local restarts invalidate stale assets automatically.
    pub app_version: String,
}

// --- env > file > default resolution helpers --------------------------------

/// Resolve a string: env var, else file value, else default.
fn resolve_str(env: &dyn EnvSource, key: &str, file: Option<&str>, default: &str) -> String {
    env.get(key)
        .or_else(|| file.map(str::to_string))
        .unwrap_or_else(|| default.to_string())
}

/// Resolve an optional string: env var, else file value, else `None`.
fn resolve_opt(env: &dyn EnvSource, key: &str, file: Option<&str>) -> Option<String> {
    env.get(key).or_else(|| file.map(str::to_string))
}

fn resolve_u16(
    env: &dyn EnvSource,
    key: &str,
    file: Option<u16>,
    default: u16,
) -> Result<u16, ConfigError> {
    match env.get(key) {
        Some(raw) => raw.parse().map_err(|_| invalid(key, &raw, "port")),
        None => Ok(file.unwrap_or(default)),
    }
}

fn resolve_u32(
    env: &dyn EnvSource,
    key: &str,
    file: Option<u32>,
    default: u32,
) -> Result<u32, ConfigError> {
    match env.get(key) {
        Some(raw) => raw.parse().map_err(|_| invalid(key, &raw, "integer")),
        None => Ok(file.unwrap_or(default)),
    }
}

fn resolve_u64(
    env: &dyn EnvSource,
    key: &str,
    file: Option<u64>,
    default: u64,
) -> Result<u64, ConfigError> {
    match env.get(key) {
        Some(raw) => raw.parse().map_err(|_| invalid(key, &raw, "integer")),
        None => Ok(file.unwrap_or(default)),
    }
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn resolve_bool(
    env: &dyn EnvSource,
    key: &str,
    file: Option<bool>,
    default: bool,
) -> Result<bool, ConfigError> {
    match env.get(key) {
        Some(raw) => parse_bool(&raw).ok_or_else(|| invalid(key, &raw, "boolean")),
        None => Ok(file.unwrap_or(default)),
    }
}

// --- section loaders --------------------------------------------------------

fn load_database(env: &dyn EnvSource, file: &FileDatabase) -> Result<DatabaseConfig, ConfigError> {
    // SQLite file in the working directory is the zero-config default.
    let default_url = "sqlite://platform_inspector.db?mode=rwc";
    Ok(DatabaseConfig {
        url: resolve_str(env, "DATABASE_URL", file.url.as_deref(), default_url),
        max_connections: resolve_u32(env, "DATABASE_MAX_CONNECTIONS", file.max_connections, 10)?,
    })
}

fn load_server(env: &dyn EnvSource, file: &FileServer) -> Result<ServerConfig, ConfigError> {
    Ok(ServerConfig {
        bind_address: resolve_str(env, "BIND_ADDRESS", file.bind_address.as_deref(), "0.0.0.0"),
        port: resolve_u16(env, "PORT", file.port, 8080)?,
        assets_dir: resolve_str(env, "ASSETS_DIR", file.assets_dir.as_deref(), "assets"),
    })
}

fn load_redis(env: &dyn EnvSource, file: &FileRedis) -> Result<RedisConfig, ConfigError> {
    Ok(RedisConfig {
        enabled: resolve_bool(env, "REDIS_ENABLED", file.enabled, false)?,
        url: resolve_str(env, "REDIS_URL", file.url.as_deref(), "redis://localhost:6379"),
    })
}

fn load_auth(env: &dyn EnvSource, file: &FileAuth) -> AuthConfig {
    let provider = resolve_opt(env, "AUTH_PROVIDER", file.provider.as_deref())
        .map(|s| AuthProvider::parse(&s))
        .unwrap_or(AuthProvider::Admin);
    let github = (provider == AuthProvider::Github).then(|| load_github(env, &file.github));
    AuthConfig {
        provider,
        admin_user: resolve_opt(env, "ADMIN_USER", file.admin_user.as_deref()),
        admin_pass: resolve_opt(env, "ADMIN_PASS", file.admin_pass.as_deref()),
        session_secret: resolve_str(
            env,
            "SESSION_SECRET",
            file.session_secret.as_deref(),
            "dev-insecure-session-secret-change-me-please",
        ),
        encryption_key: resolve_opt(env, "ENCRYPTION_KEY", file.encryption_key.as_deref())
            .unwrap_or_else(base64_default_key),
        github,
    }
}

/// Resolve a comma-separated list: env var (split on `,`) else the file list.
fn resolve_list(env: &dyn EnvSource, key: &str, file: Option<&Vec<String>>) -> Vec<String> {
    if let Some(raw) = env.get(key) {
        return raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    file.cloned().unwrap_or_default()
}

fn load_github(env: &dyn EnvSource, file: &FileGitHub) -> GitHubAuthConfig {
    let mode = resolve_opt(env, "GITHUB_AUTH_MODE", file.mode.as_deref())
        .map(|s| GitHubAuthMode::parse(&s))
        .unwrap_or(GitHubAuthMode::OauthApp);
    GitHubAuthConfig {
        mode,
        client_id: resolve_opt(env, "GITHUB_CLIENT_ID", file.client_id.as_deref()),
        client_secret: resolve_opt(env, "GITHUB_CLIENT_SECRET", file.client_secret.as_deref()),
        redirect_url: resolve_opt(env, "GITHUB_REDIRECT_URL", file.redirect_url.as_deref()),
        api_base_url: resolve_str(
            env,
            "GITHUB_API_BASE_URL",
            file.api_base_url.as_deref(),
            "https://api.github.com",
        ),
        web_base_url: resolve_str(
            env,
            "GITHUB_WEB_BASE_URL",
            file.web_base_url.as_deref(),
            "https://github.com",
        ),
        allowed_orgs: resolve_list(env, "GITHUB_ALLOWED_ORGS", file.allowed_orgs.as_ref()),
        allowed_logins: resolve_list(env, "GITHUB_ALLOWED_LOGINS", file.allowed_logins.as_ref()),
    }
}

/// A deterministic, clearly-insecure default key for local development only.
fn base64_default_key() -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode([0u8; 32])
}

impl Config {
    /// Load configuration from the environment only (no file). Kept for tests
    /// and the env-only path; the file-aware path is [`ConfigLoader`].
    pub fn load(env: &dyn EnvSource) -> Result<Self, ConfigError> {
        Self::build(env, &FileConfig::default())
    }

    /// Convenience loader reading from the real process environment only.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&SystemEnv)
    }

    /// Build a [`Config`] resolving each field as env var > file value > default.
    fn build(env: &dyn EnvSource, file: &FileConfig) -> Result<Self, ConfigError> {
        Ok(Self {
            database: load_database(env, &file.database)?,
            server: load_server(env, &file.server)?,
            auth: load_auth(env, &file.auth),
            redis: load_redis(env, &file.redis)?,
            log_level: resolve_str(env, "LOG_LEVEL", file.log.level.as_deref(), "info"),
            workspace_dir: resolve_str(
                env,
                "WORKSPACE_DIR",
                file.workspace_dir.as_deref(),
                "tmp/workspace",
            ),
            git_min_interval_ms: resolve_u64(
                env,
                "GIT_API_MIN_INTERVAL_MS",
                file.git_min_interval_ms,
                250,
            )?,
            app_version: resolve_opt(env, "APP_VERSION", file.app_version.as_deref())
                .unwrap_or_else(|| Uuid::new_v4().simple().to_string()),
        })
    }
}

// --- optional config.yaml file ---------------------------------------------

/// Deserialised `config.yaml`. Every field is optional and falls through to the
/// environment/defaults. Unknown keys are ignored so forward-looking blocks do
/// not break older binaries.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    server: FileServer,
    database: FileDatabase,
    redis: FileRedis,
    auth: FileAuth,
    log: FileLog,
    workspace_dir: Option<String>,
    git_min_interval_ms: Option<u64>,
    app_version: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileServer {
    bind_address: Option<String>,
    port: Option<u16>,
    assets_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileDatabase {
    url: Option<String>,
    max_connections: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileRedis {
    enabled: Option<bool>,
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileAuth {
    provider: Option<String>,
    admin_user: Option<String>,
    admin_pass: Option<String>,
    session_secret: Option<String>,
    encryption_key: Option<String>,
    github: FileGitHub,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileGitHub {
    mode: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_url: Option<String>,
    api_base_url: Option<String>,
    web_base_url: Option<String>,
    allowed_orgs: Option<Vec<String>>,
    allowed_logins: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileLog {
    level: Option<String>,
}

/// Loads [`Config`] from an optional `config.yaml`, layered over the
/// environment. File and environment access are injected so loading is testable.
pub struct ConfigLoader<'a> {
    pub fs: &'a dyn FileSystem,
    pub env: &'a dyn EnvSource,
}

impl ConfigLoader<'_> {
    /// Resolve the config file (CLI override, else binary-adjacent, else cwd),
    /// read + interpolate it, then build the layered [`Config`].
    pub fn load(
        &self,
        cli_path: Option<&str>,
        exe_dir: Option<&str>,
    ) -> Result<Config, ConfigError> {
        let file = match self.resolve_path(cli_path, exe_dir)? {
            Some(path) => self.read_file(&path)?,
            None => FileConfig::default(),
        };
        Config::build(self.env, &file)
    }

    /// Pick the config path. An explicit `--config-file` that is missing is a
    /// hard error; a missing default-location file is simply absent.
    fn resolve_path(
        &self,
        cli_path: Option<&str>,
        exe_dir: Option<&str>,
    ) -> Result<Option<String>, ConfigError> {
        if let Some(path) = cli_path {
            return if self.fs.exists(path) {
                Ok(Some(path.to_string()))
            } else {
                Err(ConfigError::Invalid {
                    key: "config-file".into(),
                    reason: format!("file not found: {path}"),
                })
            };
        }
        for dir in exe_dir.into_iter().chain(std::iter::once(".")) {
            let candidate = format!("{}/config.yaml", dir.trim_end_matches(['/', '\\']));
            if self.fs.exists(&candidate) {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn read_file(&self, path: &str) -> Result<FileConfig, ConfigError> {
        let raw = self.fs.read_to_string(path).map_err(|e| ConfigError::Invalid {
            key: "config-file".into(),
            reason: e.to_string(),
        })?;
        match raw {
            Some(text) => {
                let interpolated = crate::strings::interpolate_env(&text, self.env);
                serde_yaml::from_str(&interpolated).map_err(|e| ConfigError::Invalid {
                    key: "config-file".into(),
                    reason: format!("invalid YAML in {path}: {e}"),
                })
            }
            None => Ok(FileConfig::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;

    #[test]
    fn defaults_apply_when_env_empty() {
        let cfg = Config::load(&MapEnv::new()).unwrap();
        assert!(cfg.database.url.starts_with("sqlite://"));
        assert_eq!(cfg.database.engine(), DbEngine::Sqlite);
        assert_eq!(cfg.database.max_connections, 10);
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.server.assets_dir, "assets");
        assert!(cfg.auth.admin_user.is_none());
        assert_eq!(cfg.auth.provider, AuthProvider::Admin);
        assert!(!cfg.redis.enabled);
        assert_eq!(cfg.log_level, "info");
        assert_eq!(cfg.workspace_dir, "tmp/workspace");
    }

    #[test]
    fn overrides_are_applied() {
        let env = MapEnv::new()
            .with("DATABASE_URL", "postgres://u:p@db:5432/x")
            .with("PORT", "9000")
            .with("ADMIN_USER", "root")
            .with("ADMIN_PASS", "secret");
        let cfg = Config::load(&env).unwrap();
        assert_eq!(cfg.database.url, "postgres://u:p@db:5432/x");
        assert_eq!(cfg.database.engine(), DbEngine::Postgres);
        assert_eq!(cfg.server.port, 9000);
        assert_eq!(cfg.auth.admin_user.as_deref(), Some("root"));
    }

    #[test]
    fn engine_detected_from_url() {
        assert_eq!(DbEngine::from_url("postgres://x"), DbEngine::Postgres);
        assert_eq!(DbEngine::from_url("postgresql://x"), DbEngine::Postgres);
        assert_eq!(DbEngine::from_url("sqlite://x.db"), DbEngine::Sqlite);
        assert_eq!(DbEngine::from_url("anything-else"), DbEngine::Sqlite);
    }

    #[test]
    fn app_version_uses_env_or_random_default() {
        let cfg = Config::load(&MapEnv::new().with("APP_VERSION", "1.2.3")).unwrap();
        assert_eq!(cfg.app_version, "1.2.3");

        // Absent → a non-empty value generated for this boot.
        let cfg = Config::load(&MapEnv::new()).unwrap();
        assert!(!cfg.app_version.is_empty());
    }

    #[test]
    fn invalid_port_is_rejected() {
        let env = MapEnv::new().with("PORT", "not-a-port");
        assert!(Config::load(&env).is_err());
    }

    #[test]
    fn socket_addr_formats_host_and_port() {
        let env = MapEnv::new().with("BIND_ADDRESS", "127.0.0.1").with("PORT", "1234");
        let cfg = Config::load(&env).unwrap();
        assert_eq!(cfg.server.socket_addr(), "127.0.0.1:1234");
    }

    /// A `MockFileSystem` that serves `content` for `path` and reports it exists.
    fn fs_with(path: &'static str, content: &'static str) -> MockFileSystem {
        let mut fs = MockFileSystem::new();
        fs.expect_exists()
            .returning(move |p| p == path);
        fs.expect_read_to_string()
            .returning(move |p| Ok((p == path).then(|| content.to_string())));
        fs
    }

    #[test]
    fn github_auth_config_parses_from_file() {
        let yaml = "auth:\n  provider: github\n  github:\n    mode: personal_token\n    api_base_url: https://ghe.example/api/v3\n    allowed_orgs: [acme, beta]\n    allowed_logins: [alice]\n";
        let fs = fs_with("/app/config.yaml", yaml);
        let loader = ConfigLoader { fs: &fs, env: &MapEnv::new() };
        let cfg = loader.load(Some("/app/config.yaml"), None).unwrap();
        let gh = cfg.auth.github.expect("github config present");
        assert_eq!(gh.mode, GitHubAuthMode::PersonalToken);
        assert_eq!(gh.api_base_url, "https://ghe.example/api/v3");
        assert_eq!(gh.allowed_orgs, vec!["acme", "beta"]);
        assert_eq!(gh.allowed_logins, vec!["alice"]);
    }

    #[test]
    fn github_config_absent_for_admin_provider() {
        let cfg = Config::load(&MapEnv::new()).unwrap();
        assert!(cfg.auth.github.is_none());
    }

    #[test]
    fn github_allowlist_env_overrides_file() {
        let yaml = "auth:\n  provider: github\n  github:\n    allowed_orgs: [fromfile]\n";
        let fs = fs_with("/app/config.yaml", yaml);
        let env = MapEnv::new().with("GITHUB_ALLOWED_ORGS", "a, b ,c");
        let loader = ConfigLoader { fs: &fs, env: &env };
        let cfg = loader.load(Some("/app/config.yaml"), None).unwrap();
        let gh = cfg.auth.github.unwrap();
        assert_eq!(gh.allowed_orgs, vec!["a", "b", "c"]);
    }

    #[test]
    fn file_values_apply_when_env_empty() {
        let yaml = "database:\n  url: postgres://file/db\nredis:\n  enabled: true\nauth:\n  provider: github\nlog:\n  level: debug\n";
        let fs = fs_with("/app/config.yaml", yaml);
        let loader = ConfigLoader { fs: &fs, env: &MapEnv::new() };
        let cfg = loader.load(Some("/app/config.yaml"), None).unwrap();
        assert_eq!(cfg.database.url, "postgres://file/db");
        assert!(cfg.redis.enabled);
        assert_eq!(cfg.auth.provider, AuthProvider::Github);
        assert_eq!(cfg.log_level, "debug");
    }

    #[test]
    fn env_overrides_file_value() {
        let fs = fs_with("/app/config.yaml", "database:\n  url: postgres://file/db\n");
        let env = MapEnv::new().with("DATABASE_URL", "postgres://env/db");
        let loader = ConfigLoader { fs: &fs, env: &env };
        let cfg = loader.load(Some("/app/config.yaml"), None).unwrap();
        assert_eq!(cfg.database.url, "postgres://env/db");
    }

    #[test]
    fn file_interpolates_env_references() {
        let fs = fs_with("/app/config.yaml", "database:\n  url: \"${DB_URL:-sqlite://fallback.db}\"\n");
        // Unset → uses the inline default.
        let loader = ConfigLoader { fs: &fs, env: &MapEnv::new() };
        let cfg = loader.load(Some("/app/config.yaml"), None).unwrap();
        assert_eq!(cfg.database.url, "sqlite://fallback.db");
    }

    #[test]
    fn missing_explicit_config_file_errors() {
        let mut fs = MockFileSystem::new();
        fs.expect_exists().returning(|_| false);
        let loader = ConfigLoader { fs: &fs, env: &MapEnv::new() };
        assert!(loader.load(Some("/nope/config.yaml"), None).is_err());
    }

    #[test]
    fn missing_default_file_is_ok() {
        let mut fs = MockFileSystem::new();
        fs.expect_exists().returning(|_| false);
        let loader = ConfigLoader { fs: &fs, env: &MapEnv::new() };
        let cfg = loader.load(None, Some("/app")).unwrap();
        assert_eq!(cfg.server.port, 8080);
    }

    #[test]
    fn invalid_yaml_reports_error() {
        let fs = fs_with("/app/config.yaml", "server:\n  port: [not, a, number]\n");
        let loader = ConfigLoader { fs: &fs, env: &MapEnv::new() };
        assert!(loader.load(Some("/app/config.yaml"), None).is_err());
    }
}
