//! Graceful shutdown against a real TCP listener (T0.2.1).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

/// Generous upper bound so slow CI runners do not cause flakes.
const WITHIN: Duration = Duration::from_secs(5);

struct Running {
    addr: SocketAddr,
    shutdown: oneshot::Sender<()>,
    handle: JoinHandle<io::Result<()>>,
}

async fn start(router: Router, shutdown_timeout: Duration) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown, rx) = oneshot::channel::<()>();
    let signal = async move {
        let _ = rx.await;
    };
    let handle = tokio::spawn(app::server::serve(
        listener,
        router,
        signal,
        shutdown_timeout,
    ));
    Running {
        addr,
        shutdown,
        handle,
    }
}

/// Sends a raw HTTP/1.1 request; returns the open stream so the caller decides when to read.
async fn send_request(addr: SocketAddr, path: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    stream
}

async fn read_response(mut stream: TcpStream) -> String {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    String::from_utf8(buf).unwrap()
}

fn slow_router(delay: Duration) -> Router {
    app::http::router().route(
        "/slow",
        get(move || async move {
            sleep(delay).await;
            "done"
        }),
    )
}

#[tokio::test]
async fn serves_health_over_tcp_and_stops_on_signal() {
    let server = start(app::http::router(), Duration::from_secs(5)).await;

    let response = read_response(send_request(server.addr, "/health").await).await;
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.ends_with(r#"{"status":"ok"}"#), "{response}");

    server.shutdown.send(()).unwrap();
    let result = timeout(WITHIN, server.handle).await.unwrap().unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn in_flight_request_completes_after_shutdown_signal() {
    let server = start(
        slow_router(Duration::from_millis(300)),
        Duration::from_secs(5),
    )
    .await;

    let stream = send_request(server.addr, "/slow").await;
    // Let the request reach the handler before signalling shutdown.
    sleep(Duration::from_millis(100)).await;
    server.shutdown.send(()).unwrap();

    let response = timeout(WITHIN, read_response(stream)).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.ends_with("done"), "{response}");

    let result = timeout(WITHIN, server.handle).await.unwrap().unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn new_connections_are_refused_after_shutdown() {
    let server = start(app::http::router(), Duration::from_secs(5)).await;
    let addr = server.addr;

    server.shutdown.send(()).unwrap();
    timeout(WITHIN, server.handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn gives_up_when_drain_exceeds_timeout() {
    let server = start(
        slow_router(Duration::from_secs(60)),
        Duration::from_millis(200),
    )
    .await;

    let _stream = send_request(server.addr, "/slow").await;
    sleep(Duration::from_millis(100)).await;
    server.shutdown.send(()).unwrap();

    let result = timeout(WITHIN, server.handle).await.unwrap().unwrap();
    let err = result.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}
