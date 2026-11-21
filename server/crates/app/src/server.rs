//! Serving the router with graceful shutdown.

use std::future::{Future, IntoFuture};
use std::io;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// Serves `router` on `listener` until `shutdown` resolves.
///
/// After the shutdown signal the server stops accepting new connections and waits for in-flight
/// requests to finish. If they take longer than `shutdown_timeout`, it gives up and returns an
/// [`io::ErrorKind::TimedOut`] error so the process can exit anyway.
pub async fn serve<S>(
    listener: TcpListener,
    router: Router,
    shutdown: S,
    shutdown_timeout: Duration,
) -> io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let (signalled_tx, mut signalled_rx) = watch::channel(false);
    let signal = async move {
        shutdown.await;
        // The receiver lives until `serve` returns, so this cannot fail in practice.
        let _ = signalled_tx.send(true);
    };

    // Peer addresses are needed by per-client rate limits; they are never logged.
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(signal)
    .into_future();

    let drain_deadline = async move {
        if signalled_rx.wait_for(|signalled| *signalled).await.is_err() {
            // Sender dropped without signalling: the server ended on its own; never fire.
            std::future::pending::<()>().await;
        }
        tokio::time::sleep(shutdown_timeout).await;
    };

    tokio::select! {
        result = server => result,
        () = drain_deadline => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "graceful shutdown timed out waiting for in-flight requests",
        )),
    }
}

/// Resolves on Ctrl-C or, on Unix, SIGTERM (sent by Docker/systemd on stop).
///
/// If a signal handler cannot be installed, that source is ignored rather than triggering an
/// immediate shutdown.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
