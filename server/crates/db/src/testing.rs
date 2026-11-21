//! Database test helper (feature `testing`).
//!
//! ```no_run
//! # async fn example() {
//! let db = db::testing::TestDb::new().await;
//! sqlx::query("SELECT 1").execute(db.pool()).await.unwrap();
//! # }
//! ```
//!
//! # How it works
//! - **One Postgres 17 container per test process**, started lazily on first use and shared by
//!   all tests. It is marked reusable, so later runs pick up the same container instead of
//!   starting (and leaking) new ones. Name: [`CONTAINER_NAME`].
//! - **One schema per test** (`test_<uuidv7>`): the helper creates it, applies all migrations
//!   inside it and returns a pool whose `search_path` is `<schema>, public`. Tests therefore run
//!   in parallel without seeing each other's data.
//! - Schemas left by earlier runs are dropped on first use once they are older than
//!   [`STALE_SCHEMA_AGE`].
//! - Set `TEST_DATABASE_URL` to use an existing server (e.g. the compose database) instead of a
//!   container.
//!
//! The helper panics on failure: it is only meant for tests.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt, ReuseDirective};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// Name of the shared, reusable test container.
pub const CONTAINER_NAME: &str = "secondbrainv4-test-postgres";
/// Test schemas older than this are removed on startup.
pub const STALE_SCHEMA_AGE: Duration = Duration::from_secs(60 * 60);

const SCHEMA_PREFIX: &str = "test_";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// Base connection string of the shared server, initialised once per process.
static SERVER_URL: OnceCell<String> = OnceCell::const_new();

/// An isolated schema with all migrations applied.
#[derive(Debug)]
pub struct TestDb {
    pool: PgPool,
    schema: String,
    url: String,
}

impl TestDb {
    /// Creates a fresh schema, migrates it and returns a pool bound to it.
    pub async fn new() -> Self {
        let db = Self::new_unmigrated().await;
        crate::migrate(&db.pool)
            .await
            .expect("apply migrations to test schema");
        db
    }

    /// Creates a fresh, **empty** schema without applying migrations (for testing the migration
    /// flow itself).
    pub async fn new_unmigrated() -> Self {
        let server = server_url().await;
        let schema = format!("{SCHEMA_PREFIX}{}", Uuid::now_v7().simple());

        let mut admin = PgConnection::connect(server)
            .await
            .expect("connect to test postgres");
        sqlx::query(schema_ddl("CREATE SCHEMA", &schema, ""))
            .execute(&mut admin)
            .await
            .expect("create test schema");
        admin.close().await.ok();

        let options = PgConnectOptions::from_str(server)
            .expect("parse test database url")
            .options([("search_path", format!("{schema},public"))]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect test pool");

        // Same database, same schema, as a URL (for code that takes a `DatabaseUrl`).
        let separator = if server.contains('?') { '&' } else { '?' };
        let url = format!("{server}{separator}options=-c%20search_path%3D{schema}%2Cpublic");

        Self { pool, schema, url }
    }

    /// Pool whose connections use this test's schema.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Name of this test's schema.
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Connection string that selects this test's schema via `search_path`.
    pub fn url(&self) -> &str {
        &self.url
    }
}

async fn server_url() -> &'static str {
    SERVER_URL
        .get_or_init(|| async {
            let url = match std::env::var("TEST_DATABASE_URL") {
                Ok(url) if !url.trim().is_empty() => url,
                _ => start_container().await,
            };
            wait_until_ready(&url).await;
            drop_stale_schemas(&url).await;
            url
        })
        .await
}

async fn start_container() -> String {
    let container = GenericImage::new("postgres", "17")
        .with_exposed_port(5432.tcp())
        // Readiness is checked by connecting over TCP (see `wait_until_ready`); the log line
        // "ready to accept connections" is printed first by the entrypoint's temporary,
        // socket-only init server and would report ready too early.
        .with_wait_for(WaitFor::Nothing)
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "postgres")
        // Durability is irrelevant for throwaway test data; this makes tests much faster.
        .with_cmd([
            "postgres",
            "-c",
            "fsync=off",
            "-c",
            "synchronous_commit=off",
            "-c",
            "full_page_writes=off",
        ])
        .with_container_name(CONTAINER_NAME)
        .with_reuse(ReuseDirective::Always)
        .with_startup_timeout(STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start postgres test container (is Docker running?)");

    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");

    // Reusable containers are not removed on drop; the handle must not outlive the runtime of
    // the test that started it, so release it here.
    drop(container);

    format!("postgres://postgres:postgres@{host}:{port}/postgres")
}

async fn wait_until_ready(url: &str) {
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    loop {
        match PgConnection::connect(url).await {
            Ok(conn) => {
                conn.close().await.ok();
                return;
            }
            Err(err) if tokio::time::Instant::now() < deadline => {
                let _ = err;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(err) => panic!("test postgres not ready after {STARTUP_TIMEOUT:?}: {err}"),
        }
    }
}

/// Drops `test_<uuidv7>` schemas whose embedded timestamp is older than [`STALE_SCHEMA_AGE`].
async fn drop_stale_schemas(url: &str) {
    let mut conn = PgConnection::connect(url)
        .await
        .expect("connect for cleanup");
    let schemas: Vec<String> =
        sqlx::query_scalar("SELECT nspname::text FROM pg_namespace WHERE nspname LIKE 'test\\_%'")
            .fetch_all(&mut conn)
            .await
            .expect("list test schemas");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let max_age_ms = STALE_SCHEMA_AGE.as_millis();

    for schema in schemas {
        if is_test_schema_name(&schema) && is_stale(&schema, now_ms, max_age_ms) {
            sqlx::query(schema_ddl("DROP SCHEMA IF EXISTS", &schema, " CASCADE"))
                .execute(&mut conn)
                .await
                .ok();
        }
    }
    conn.close().await.ok();
}

/// Builds a DDL statement for a test schema. Identifiers cannot be bind parameters, so the name
/// is interpolated; it is safe because only `test_<hex uuid>` names are accepted.
fn schema_ddl(prefix: &str, schema: &str, suffix: &str) -> sqlx::AssertSqlSafe<String> {
    assert!(
        is_test_schema_name(schema),
        "refusing to build DDL for unexpected schema name {schema:?}"
    );
    sqlx::AssertSqlSafe(format!(r#"{prefix} "{schema}"{suffix}"#))
}

fn is_test_schema_name(schema: &str) -> bool {
    schema
        .strip_prefix(SCHEMA_PREFIX)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_stale(schema: &str, now_ms: u128, max_age_ms: u128) -> bool {
    let Some(created_ms) = schema
        .strip_prefix(SCHEMA_PREFIX)
        .and_then(|hex| Uuid::try_parse(hex).ok())
        .and_then(|uuid| uuid.get_timestamp())
        .map(|ts| {
            let (secs, nanos) = ts.to_unix();
            u128::from(secs) * 1000 + u128::from(nanos / 1_000_000)
        })
    else {
        return false; // not one of ours
    };
    now_ms.saturating_sub(created_ms) > max_age_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_detection_uses_uuidv7_timestamp() {
        let fresh = format!("{SCHEMA_PREFIX}{}", Uuid::now_v7().simple());
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let hour = STALE_SCHEMA_AGE.as_millis();

        assert!(!is_stale(&fresh, now_ms, hour));
        assert!(is_stale(&fresh, now_ms + 2 * hour, hour));
        assert!(!is_stale("test_not_a_uuid", now_ms, hour));
        assert!(!is_stale("public", now_ms, hour));
    }

    #[test]
    fn only_generated_names_are_accepted_for_ddl() {
        assert!(is_test_schema_name(&format!(
            "{SCHEMA_PREFIX}{}",
            Uuid::now_v7().simple()
        )));
        assert!(!is_test_schema_name("test_"));
        assert!(!is_test_schema_name("public"));
        assert!(!is_test_schema_name(r#"test_abc"; DROP SCHEMA public; --"#));
    }

    #[test]
    #[should_panic(expected = "refusing to build DDL")]
    fn ddl_rejects_unexpected_names() {
        let _ = schema_ddl("DROP SCHEMA", "public", " CASCADE");
    }
}
