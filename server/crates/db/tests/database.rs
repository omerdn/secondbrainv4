//! `db` against a real Postgres 17 (T0.2.3). Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use db::testing::TestDb;
use db::{DatabaseUrl, PoolConfig};

async fn table_exists(pool: &db::PgPool, name: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn migrations_are_applied_in_the_test_schema() {
    let db = TestDb::new().await;

    let schema: String = sqlx::query_scalar("SELECT current_schema()::text")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(schema, db.schema());

    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(applied, i64::try_from(db::MIGRATOR.iter().count()).unwrap());
}

#[tokio::test]
async fn search_extensions_are_available() {
    let db = TestDb::new().await;

    let folded: String = sqlx::query_scalar("SELECT unaccent('Çalışma Öğün')")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(folded, "Calisma Ogun");

    let similar: bool = sqlx::query_scalar("SELECT similarity('rutin', 'rutinler') > 0.3")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert!(similar);
}

#[tokio::test]
async fn each_test_db_is_isolated() {
    let a = TestDb::new().await;
    let b = TestDb::new().await;
    assert_ne!(a.schema(), b.schema());

    sqlx::query("CREATE TABLE notes (body text)")
        .execute(a.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO notes VALUES ('only in a')")
        .execute(a.pool())
        .await
        .unwrap();

    assert!(table_exists(a.pool(), "notes").await);
    assert!(!table_exists(b.pool(), "notes").await);
}

#[tokio::test]
async fn migrate_is_idempotent() {
    let db = TestDb::new().await;
    db::migrate(db.pool()).await.unwrap();
    db::migrate(db.pool()).await.unwrap();
}

#[tokio::test]
async fn connect_and_ping_through_the_public_api() {
    let test_db = TestDb::new().await;
    let pool = db::connect(&PoolConfig::new(DatabaseUrl::new(test_db.url())))
        .await
        .unwrap();
    db::ping(&pool).await.unwrap();

    // The URL form selects the same schema as the helper's pool.
    let schema: String = sqlx::query_scalar("SELECT current_schema()::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(schema, test_db.schema());
}

#[tokio::test]
async fn connect_fails_fast_for_unreachable_database() {
    let mut config = PoolConfig::new(DatabaseUrl::new(
        "postgres://nobody:secret@127.0.0.1:1/nowhere",
    ));
    config.acquire_timeout = std::time::Duration::from_secs(2);
    assert!(db::connect(&config).await.is_err());
}
