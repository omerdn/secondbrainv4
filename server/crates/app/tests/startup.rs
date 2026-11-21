//! `app::run` end to end with a real database (T0.2.3): connect, migrate on start, serve, stop.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use app::{Config, RunError};
use db::testing::TestDb;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::timeout;

const WITHIN: Duration = Duration::from_secs(10);

fn config_for(url: &str, migrate_on_start: bool) -> Config {
    let migrate = migrate_on_start.to_string();
    let vars = [
        ("DATABASE_URL", url),
        ("DB_MIGRATE_ON_START", migrate.as_str()),
        ("SHUTDOWN_TIMEOUT_SECS", "5"),
    ];
    Config::from_lookup(|key| {
        vars.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_owned())
    })
    .unwrap()
}

async fn get_health(addr: std::net::SocketAddr) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).await.unwrap();
    buf
}

/// Applied migrations in the test schema (0 if the history table does not exist yet).
async fn applied_migrations(db: &TestDb) -> i64 {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(db.pool())
        .await
        .unwrap();
    if !exists {
        return 0;
    }
    sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(db.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn starts_migrates_serves_and_stops() {
    // Start from an empty schema to prove `run` applies the migrations.
    let test_db = TestDb::new_unmigrated().await;
    assert_eq!(applied_migrations(&test_db).await, 0);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app::run(
        config_for(test_db.url(), true),
        listener,
        async move {
            let _ = stopped.await;
        },
    ));

    let response = timeout(WITHIN, get_health(addr)).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(
        applied_migrations(&test_db).await,
        i64::try_from(db::MIGRATOR.iter().count()).unwrap()
    );

    stop.send(()).unwrap();
    timeout(WITHIN, handle).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn runs_the_outbox_dispatcher_in_the_background() {
    let test_db = TestDb::new().await;
    let event_id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO outbox (id, event_type, payload) VALUES ($1, 'test.ping', '{}')")
        .bind(event_id)
        .execute(test_db.pool())
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app::run(
        config_for(test_db.url(), true),
        listener,
        async move {
            let _ = stopped.await;
        },
    ));

    // No subscriber for `test.ping`: the dispatcher marks it dispatched.
    let dispatched = timeout(WITHIN, async {
        loop {
            let done: bool =
                sqlx::query_scalar("SELECT dispatched_at IS NOT NULL FROM outbox WHERE id = $1")
                    .bind(event_id)
                    .fetch_one(test_db.pool())
                    .await
                    .unwrap();
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(dispatched.is_ok(), "event not dispatched by app::run");

    stop.send(()).unwrap();
    timeout(WITHIN, handle).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn migrate_on_start_can_be_disabled() {
    let test_db = TestDb::new_unmigrated().await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app::run(
        config_for(test_db.url(), false),
        listener,
        async move {
            let _ = stopped.await;
        },
    ));
    stop.send(()).unwrap();
    timeout(WITHIN, handle).await.unwrap().unwrap().unwrap();

    assert_eq!(applied_migrations(&test_db).await, 0);
}

#[tokio::test]
async fn fails_when_database_is_unreachable() {
    let mut config = config_for("postgres://nobody:secret@127.0.0.1:1/nowhere", true);
    config.database.acquire_timeout = Duration::from_secs(2);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    let result = timeout(WITHIN, app::run(config, listener, std::future::pending()))
        .await
        .unwrap();
    let err = result.unwrap_err();
    assert!(matches!(err, RunError::Database(_)), "{err}");
    assert!(!err.to_string().contains("secret"));
}
