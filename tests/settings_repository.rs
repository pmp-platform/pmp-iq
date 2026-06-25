//! Integration test for the settings repository against both engines:
//! PostgreSQL (testcontainers) and SQLite (temp file).

mod common;

use common::{SqliteDb, TestDb};
use platform_inspector::db::Database;
use platform_inspector::store;
use serde_json::json;

async fn round_trip(db: Database) {
    let repo = store::settings(&db);

    assert_eq!(repo.get("missing").await.unwrap(), None);

    repo.set("ui", &json!({"theme": "dark"})).await.unwrap();
    assert_eq!(repo.get("ui").await.unwrap(), Some(json!({"theme": "dark"})));

    // Upsert overwrites.
    repo.set("ui", &json!({"theme": "light"})).await.unwrap();
    assert_eq!(repo.get("ui").await.unwrap(), Some(json!({"theme": "light"})));
}

#[tokio::test]
async fn settings_round_trip_postgres() {
    let db = TestDb::start().await;
    round_trip(db.database()).await;
}

#[tokio::test]
async fn settings_round_trip_sqlite() {
    let db = SqliteDb::start().await;
    round_trip(db.database()).await;
}
