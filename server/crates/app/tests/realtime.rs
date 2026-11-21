//! `app::run` serves the realtime channel (T0.5.1): `/ws` exists and requires a session.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use db::testing::TestDb;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::timeout;

#[tokio::test]
async fn ws_route_is_mounted_and_authenticated() {
    let test_db = TestDb::new().await;
    let vars = [
        ("DATABASE_URL", test_db.url()),
        ("DB_MIGRATE_ON_START", "false"),
        ("SHUTDOWN_TIMEOUT_SECS", "5"),
    ];
    let config = app::Config::from_lookup(|key| {
        vars.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_owned())
    })
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app::run(config, listener, async move {
        let _ = stopped.await;
    }));

    let response = timeout(Duration::from_secs(10), async {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                b"GET /ws HTTP/1.1\r\nHost: t\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
                  Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
            )
            .await
            .unwrap();
        let mut buf = vec![0; 512];
        let n = stream.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    })
    .await
    .unwrap();
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");

    stop.send(()).unwrap();
    timeout(Duration::from_secs(10), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
