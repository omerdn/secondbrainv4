//! The link preview proxy against a local web server (T2.5.8, PRD Y4): SSRF guards on inside
//! addresses and redirects, the size and time limits, and what is read from a page.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use notebooks::embeds::{
    FETCH_TIMEOUT, FetchPolicy, MAX_BODY_BYTES, Preview, PreviewError, preview,
};
use tokio::net::TcpListener;

const LOOPBACK: FetchPolicy = FetchPolicy {
    allow_loopback: true,
};

fn html(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

fn redirect(to: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, to.to_owned())]).into_response()
}

async fn site() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/page",
            get(|| async {
                html(
                    r#"<html><head><title>Yedek</title>
                    <meta property="og:title" content="Gitar dersleri">
                    <meta name="description" content="Barre akorları">
                    </head></html>"#
                        .to_owned(),
                )
            }),
        )
        .route("/to-page", get(|| async { redirect("/page") }))
        .route(
            "/to-metadata",
            get(|| async { redirect("http://169.254.169.254/latest/meta-data/") }),
        )
        .route(
            "/to-localhost",
            get(|| async { redirect("http://localhost/admin") }),
        )
        .route("/loop", get(|| async { redirect("/loop") }))
        .route(
            "/huge",
            get(|| async {
                // The title only comes after the size limit.
                html(format!(
                    "<html><head>{}<title>Geç başlık</title></head></html>",
                    " ".repeat(MAX_BODY_BYTES * 2)
                ))
            }),
        )
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(FETCH_TIMEOUT * 3).await;
                html("<title>Geç</title>".to_owned())
            }),
        )
        .route(
            "/json",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    "{\"title\":\"x\"}",
                )
                    .into_response()
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn link(preview: Preview) -> (Option<String>, Option<String>) {
    match preview {
        Preview::Link {
            title, description, ..
        } => (title, description),
        Preview::Embed { .. } => panic!("not a link"),
    }
}

#[tokio::test]
async fn ssrf_inside_addresses_are_never_fetched() {
    let addr = site().await;
    for url in [
        format!("http://{addr}/page"),
        format!("http://localhost:{}/page", addr.port()),
        "http://169.254.169.254/latest/meta-data/".to_owned(),
        "http://10.0.0.1/".to_owned(),
        "http://[::1]/".to_owned(),
        "http://0.0.0.0/".to_owned(),
    ] {
        assert_eq!(
            preview(&url, FetchPolicy::PUBLIC).await,
            Err(PreviewError::Blocked),
            "{url}"
        );
    }
}

#[tokio::test]
async fn ssrf_redirects_are_checked_again_and_limited() {
    let addr = site().await;
    // Allowed: a redirect within the (test) site.
    let (title, description) = link(
        preview(&format!("http://{addr}/to-page"), LOOPBACK)
            .await
            .unwrap(),
    );
    assert_eq!(title.as_deref(), Some("Gitar dersleri"));
    assert_eq!(description.as_deref(), Some("Barre akorları"));
    // A redirect to the cloud metadata address or to another local name is refused.
    assert_eq!(
        preview(&format!("http://{addr}/to-metadata"), LOOPBACK).await,
        Err(PreviewError::Blocked)
    );
    let to_localhost = preview(&format!("http://{addr}/to-localhost"), LOOPBACK).await;
    assert!(to_localhost.is_err(), "{to_localhost:?}");
    // Endless redirects stop.
    assert_eq!(
        preview(&format!("http://{addr}/loop"), LOOPBACK).await,
        Err(PreviewError::Unreachable)
    );
}

#[tokio::test]
async fn ssrf_size_and_time_are_bounded() {
    let addr = site().await;
    // Only the first MAX_BODY_BYTES are read: the late title is not seen.
    let (title, _) = link(
        preview(&format!("http://{addr}/huge"), LOOPBACK)
            .await
            .unwrap(),
    );
    assert_eq!(title, None);
    // A server that never answers is given up on in time.
    let started = Instant::now();
    assert_eq!(
        preview(&format!("http://{addr}/slow"), LOOPBACK).await,
        Err(PreviewError::Unreachable)
    );
    assert!(started.elapsed() < FETCH_TIMEOUT + Duration::from_secs(2));
    // Only HTML is read.
    assert_eq!(
        preview(&format!("http://{addr}/json"), LOOPBACK).await,
        Err(PreviewError::Unreachable)
    );
}

#[tokio::test]
async fn y4_embeds_need_no_fetch() {
    // Even with no network, an allowed provider's link becomes an embed.
    let embed = preview("https://youtu.be/dQw4w9WgXcQ", FetchPolicy::PUBLIC)
        .await
        .unwrap();
    assert!(matches!(embed, Preview::Embed { .. }));
}
