//! Application configuration loaded from the environment.
//!
//! Environment access is abstracted behind [`EnvSource`] so configuration
//! loading can be unit-tested without touching the real process environment.

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

/// Database connection settings. SQLite is the zero-config default;
/// PostgreSQL is used when `DATABASE_URL` points at a `postgres://` URL.
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

/// Authentication / session settings.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub admin_user: Option<String>,
    pub admin_pass: Option<String>,
    pub session_secret: String,
    /// Key used to encrypt secrets at rest (base64, 32 bytes once decoded).
    pub encryption_key: String,
}

/// Top-level application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub workspace_dir: String,
    /// Minimum interval (ms) between outbound git-provider API calls (throttle).
    pub git_min_interval_ms: u64,
    /// Cache-busting version appended to asset URLs (`?v=`). Taken from
    /// `APP_VERSION` when set, otherwise a random value generated per boot so
    /// local restarts invalidate stale assets automatically.
    pub app_version: String,
}

fn parse_u32(env: &dyn EnvSource, key: &str, default: u32) -> Result<u32, ConfigError> {
    match env.get(key) {
        None => Ok(default),
        Some(raw) => raw.parse().map_err(|_| ConfigError::Invalid {
            key: key.to_string(),
            reason: format!("'{raw}' is not a valid integer"),
        }),
    }
}

fn parse_u64(env: &dyn EnvSource, key: &str, default: u64) -> Result<u64, ConfigError> {
    match env.get(key) {
        None => Ok(default),
        Some(raw) => raw.parse().map_err(|_| ConfigError::Invalid {
            key: key.to_string(),
            reason: format!("'{raw}' is not a valid integer"),
        }),
    }
}

fn parse_u16(env: &dyn EnvSource, key: &str, default: u16) -> Result<u16, ConfigError> {
    match env.get(key) {
        None => Ok(default),
        Some(raw) => raw.parse().map_err(|_| ConfigError::Invalid {
            key: key.to_string(),
            reason: format!("'{raw}' is not a valid port"),
        }),
    }
}

fn load_database(env: &dyn EnvSource) -> Result<DatabaseConfig, ConfigError> {
    // SQLite file in the working directory is the zero-config default.
    let default_url = "sqlite://platform_inspector.db?mode=rwc";
    Ok(DatabaseConfig {
        url: env.get("DATABASE_URL").unwrap_or_else(|| default_url.into()),
        max_connections: parse_u32(env, "DATABASE_MAX_CONNECTIONS", 10)?,
    })
}

fn load_server(env: &dyn EnvSource) -> Result<ServerConfig, ConfigError> {
    Ok(ServerConfig {
        bind_address: env.get("BIND_ADDRESS").unwrap_or_else(|| "0.0.0.0".into()),
        port: parse_u16(env, "PORT", 8080)?,
        assets_dir: env.get("ASSETS_DIR").unwrap_or_else(|| "assets".into()),
    })
}

fn load_auth(env: &dyn EnvSource) -> AuthConfig {
    AuthConfig {
        admin_user: env.get("ADMIN_USER"),
        admin_pass: env.get("ADMIN_PASS"),
        session_secret: env
            .get("SESSION_SECRET")
            .unwrap_or_else(|| "dev-insecure-session-secret-change-me-please".into()),
        encryption_key: env.get("ENCRYPTION_KEY").unwrap_or_else(base64_default_key),
    }
}

/// A deterministic, clearly-insecure default key for local development only.
fn base64_default_key() -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode([0u8; 32])
}

impl Config {
    /// Load configuration from the given environment source.
    pub fn load(env: &dyn EnvSource) -> Result<Self, ConfigError> {
        Ok(Self {
            database: load_database(env)?,
            server: load_server(env)?,
            auth: load_auth(env),
            workspace_dir: env.get("WORKSPACE_DIR").unwrap_or_else(|| "workspace".into()),
            git_min_interval_ms: parse_u64(env, "GIT_API_MIN_INTERVAL_MS", 250)?,
            app_version: env
                .get("APP_VERSION")
                .unwrap_or_else(|| Uuid::new_v4().simple().to_string()),
        })
    }

    /// Convenience loader reading from the real process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&SystemEnv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_env_empty() {
        let cfg = Config::load(&MapEnv::new()).unwrap();
        assert!(cfg.database.url.starts_with("sqlite://"));
        assert_eq!(cfg.database.engine(), DbEngine::Sqlite);
        assert_eq!(cfg.database.max_connections, 10);
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.server.assets_dir, "assets");
        assert!(cfg.auth.admin_user.is_none());
        assert_eq!(cfg.workspace_dir, "workspace");
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
}
