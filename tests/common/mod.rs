//! Shared integration-test harness: a disposable PostgreSQL container with the
//! project's dbmate migrations applied.
//!
//! Each test that needs a database calls [`TestDb::start`]. The returned guard
//! owns the container; dropping it tears the container down.

#![allow(dead_code)]

use platform_inspector::app::AppState;
use platform_inspector::auth::{Argon2Hasher, AuthService, RandomSecretGenerator};
use platform_inspector::config::{Config, MapEnv};
use platform_inspector::db::{Database, migrate};
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{PgPool, SqlitePool};
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;

/// Build application state for a given database handle, with a known admin
/// (`admin` / `admin`).
pub fn build_state_db(db: Database) -> AppState {
    let workspace = std::env::temp_dir()
        .join(format!("pi-workspace-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let env = MapEnv::new()
        .with("ADMIN_USER", "admin")
        .with("ADMIN_PASS", "admin")
        .with("WORKSPACE_DIR", &workspace);
    let config = Config::load(&env).unwrap();
    let boot =
        AuthService::from_config(&config.auth, Arc::new(Argon2Hasher), &RandomSecretGenerator)
            .unwrap();
    AppState::build(config, db, Arc::new(boot.service)).unwrap()
}

/// Build application state backed by the Postgres test container.
pub fn build_state(db: &TestDb) -> AppState {
    build_state_db(db.database())
}

/// Build application state backed by a SQLite test database.
pub fn build_state_sqlite(db: &SqliteDb) -> AppState {
    build_state_db(db.database())
}

/// Collect `Set-Cookie` values from a response into `name=value` cookie pairs.
pub fn extract_cookies(resp: &axum::response::Response) -> Vec<String> {
    resp.headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|s| s.split(';').next())
        .map(|s| s.to_string())
        .collect()
}

/// Join cookie pairs into a `Cookie` request header value.
pub fn cookie_header(cookies: &[String]) -> String {
    cookies.join("; ")
}

/// Extract the CSRF hidden-field token from login-page HTML.
pub fn extract_csrf(html: &str) -> String {
    let marker = "name=\"csrf\" value=\"";
    let start = html.find(marker).expect("csrf field present") + marker.len();
    let rest = &html[start..];
    let end = rest.find('"').expect("csrf value terminator");
    rest[..end].to_string()
}

/// Log in via the HTTP flow and return the session cookies for reuse.
pub async fn login_cookies(
    app: &axum::Router,
    username: &str,
    password: &str,
) -> Vec<String> {
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::header::{CONTENT_TYPE, COOKIE};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let get = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookies = extract_cookies(&get);
    let html = {
        let bytes = get.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    };
    let csrf = extract_csrf(&html);
    let body = serde_urlencoded::to_string([
        ("csrf", csrf.as_str()),
        ("username", username),
        ("password", password),
    ])
    .unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let post_cookies = extract_cookies(&resp);
    if post_cookies.is_empty() { cookies } else { post_cookies }
}

/// A running PostgreSQL test container plus a connected pool.
pub struct TestDb {
    _container: ContainerAsync<Postgres>,
    pub pool: PgPool,
}

impl TestDb {
    /// Start a fresh PostgreSQL container and apply all migrations.
    pub async fn start() -> Self {
        let container = Postgres::default()
            .with_tag("16-alpine")
            .start()
            .await
            .expect("start postgres container");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("map postgres port");
        let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect to postgres container");

        let db = Database::from_pg(pool.clone());
        migrate::apply(&db, migrate::PG_MIGRATIONS)
            .await
            .expect("apply postgres migrations");

        Self {
            _container: container,
            pool,
        }
    }

    /// A `Database` handle backed by the container pool.
    pub fn database(&self) -> Database {
        Database::from_pg(self.pool.clone())
    }
}

/// A SQLite test database backed by a temp file (no container needed).
pub struct SqliteDb {
    pub pool: SqlitePool,
    path: std::path::PathBuf,
}

impl SqliteDb {
    /// Create a fresh SQLite database with all migrations applied.
    pub async fn start() -> Self {
        let path = std::env::temp_dir().join(format!("pi-sqlite-{}.db", uuid::Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.to_string_lossy());
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("connect to sqlite");

        let db = Database::from_sqlite(pool.clone());
        migrate::apply(&db, migrate::SQLITE_MIGRATIONS)
            .await
            .expect("apply sqlite migrations");

        Self { pool, path }
    }

    pub fn database(&self) -> Database {
        Database::from_sqlite(self.pool.clone())
    }
}

impl Drop for SqliteDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
