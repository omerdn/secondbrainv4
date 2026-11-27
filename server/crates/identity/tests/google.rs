//! Google sign-in (T0.3.4) against a real Postgres 17 and a fake Google token endpoint.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Form, State};
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::oauth::google::{Google, GoogleConfig, STATE_TTL};
use identity::{EmailSettings, IdentityService};
use kernel::email::RecordingEmailSender;
use kernel::{Clock, FixedClock};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

/// A Google account the fake provider knows, keyed by authorization code.
#[derive(Clone)]
struct Account {
    sub: &'static str,
    email: &'static str,
    email_verified: bool,
}

#[derive(Clone, Default)]
struct FakeGoogle {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
    /// Form fields of every token request.
    requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    exp: i64,
}

async fn token_endpoint(
    State(fake): State<FakeGoogle>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    fake.requests.lock().unwrap().push(form.clone());
    let code = form.get("code").cloned().unwrap_or_default();
    let Some(account) = fake.accounts.lock().unwrap().get(&code).cloned() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant" })),
        )
            .into_response();
    };
    // Native requests carry client_id in the form; the web client authenticates with Basic auth.
    let aud = form
        .get("client_id")
        .cloned()
        .unwrap_or_else(|| "web-id".into());
    let claims = json!({
        "iss": "https://accounts.google.com", "aud": aud, "sub": account.sub,
        "exp": fake.exp, "email": account.email, "email_verified": account.email_verified,
        "name": "Ada Google", "picture": "https://img.test/a.png"
    });
    let id_token = format!(
        "{}.{}.sig",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256"}"#),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    Json(json!({ "access_token": "at", "token_type": "Bearer", "expires_in": 3600, "id_token": id_token }))
        .into_response()
}

struct App {
    db: TestDb,
    clock: FixedClock,
    fake: FakeGoogle,
    service: IdentityService,
    router: Router,
}

impl App {
    async fn new() -> Self {
        Self::build(true).await
    }

    async fn build(with_google: bool) -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap());
        let fake = FakeGoogle {
            exp: clock.now().timestamp() + 3600,
            ..FakeGoogle::default()
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake_router = Router::new()
            .route("/token", post(token_endpoint))
            .with_state(fake.clone());
        tokio::spawn(async move { axum::serve(listener, fake_router).await.unwrap() });

        let mut service = IdentityService::new(
            db.pool().clone(),
            Arc::new(clock.clone()),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        if with_google {
            service = service.with_google(
                Google::new(GoogleConfig {
                    web_client_id: "web-id".into(),
                    web_client_secret: "web-secret".into(),
                    native_client_ids: vec!["ios-id".into()],
                    callback_url: "https://api.test/auth/google/callback".into(),
                    web_url: "https://app.test".into(),
                    auth_url: "https://accounts.test/auth".into(),
                    token_url: format!("http://{addr}/token"),
                })
                .unwrap(),
            );
        }
        let router = identity::http::router(service.clone());
        Self {
            db,
            clock,
            fake,
            service,
            router,
        }
    }

    fn account(&self, code: &str, sub: &'static str, email: &'static str, email_verified: bool) {
        self.fake.accounts.lock().unwrap().insert(
            code.into(),
            Account {
                sub,
                email,
                email_verified,
            },
        );
    }

    async fn get(&self, uri: &str, cookie: Option<&str>) -> Response {
        let mut request = Request::builder().method(Method::GET).uri(uri);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn post(&self, uri: &str, bearer: Option<&str>, body: Value) -> Response {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        self.router
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    /// Runs `/start`; returns (authorization URL, state).
    async fn start(&self) -> (String, String) {
        let response = self.get("/auth/google/start", None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned();
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        let state = query_param(&location, "state");
        assert!(
            cookie.starts_with(&format!("sb_oauth_state={state};")),
            "{cookie}"
        );
        (location, state)
    }

    async fn callback(&self, code: &str, state: &str, cookie_state: Option<&str>) -> Response {
        let cookie = cookie_state.map(|s| format!("sb_oauth_state={s}"));
        self.get(
            &format!("/auth/google/callback?code={code}&state={state}"),
            cookie.as_deref(),
        )
        .await
    }

    /// The native sign-in, finished through the consent screen when it asks for one (HS5).
    async fn native(&self, code: &str, client_id: &str) -> Response {
        let response = self.native_raw(code, client_id).await;
        self.finish_consent(response, "native").await
    }

    /// Accepts the legal texts when `response` holds the sign-in for consent; else `response`.
    async fn finish_consent(&self, response: Response, client: &str) -> Response {
        if response.status() != StatusCode::CONFLICT {
            return response;
        }
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        match value["error"]["consent_ticket"].as_str() {
            Some(ticket) => {
                self.post(
                    "/auth/consent",
                    None,
                    json!({
                        "ticket": ticket,
                        "accepted": identity::legal::current_acceptances(),
                        "client": client
                    }),
                )
                .await
            }
            None => Response::from_parts(parts, Body::from(bytes)),
        }
    }

    async fn native_raw(&self, code: &str, client_id: &str) -> Response {
        self.post(
            "/auth/google/native",
            None,
            json!({
                "code": code, "code_verifier": "v".repeat(43),
                "redirect_uri": "com.example.app:/oauth", "client_id": client_id, "device": "iPhone"
            }),
        )
        .await
    }

    async fn scalar(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
}

fn query_param(url: &str, name: &str) -> String {
    url.split(['?', '&'])
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .unwrap()
        .to_owned()
}

fn location(response: &Response) -> &str {
    response.headers()[header::LOCATION].to_str().unwrap()
}

fn session_cookie(response: &Response) -> Option<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.starts_with("sb_session="))
        .map(|c| c.split(';').next().unwrap().to_owned())
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn web_flow_creates_a_verified_google_account_and_sets_the_cookie() {
    let app = App::new().await;
    app.account("code-1", "g-1", "ada@gmail.test", true);

    let (url, state) = app.start().await;
    assert!(url.starts_with("https://accounts.test/auth?"));
    assert_eq!(query_param(&url, "client_id"), "web-id");
    assert_eq!(query_param(&url, "code_challenge_method"), "S256");
    assert!(url.contains("redirect_uri=https%3A%2F%2Fapi.test%2Fauth%2Fgoogle%2Fcallback"));
    assert!(url.contains("scope=openid+email+profile"));
    let challenge = query_param(&url, "code_challenge");

    // HS5: a new account waits for consent; nothing exists before it.
    let response = app.callback("code-1", &state, Some(&state)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let consent = location(&response);
    assert!(
        consent.starts_with("https://app.test/consent?ticket="),
        "{consent}"
    );
    assert!(session_cookie(&response).is_none());
    assert_eq!(app.scalar("SELECT count(*) FROM users").await, 0);
    let ticket = query_param(consent, "ticket");
    let response = app
        .post(
            "/auth/consent",
            None,
            json!({ "ticket": ticket, "accepted": identity::legal::current_acceptances(), "client": "web" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = session_cookie(&response).expect("session cookie");

    // PKCE: the verifier sent to Google matches the challenge from /start.
    let requests = app.fake.requests.lock().unwrap().clone();
    let verifier = &requests[0]["code_verifier"];
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        challenge
    );
    assert_eq!(
        requests[0]["redirect_uri"],
        "https://api.test/auth/google/callback"
    );

    let sessions = app.get("/auth/sessions", Some(&cookie)).await;
    assert_eq!(sessions.status(), StatusCode::OK);
    assert_eq!(
        app.scalar("SELECT count(*) FROM auth_identities WHERE provider = 'google' AND provider_uid = 'g-1'").await,
        1
    );
    assert_eq!(
        app.scalar("SELECT count(*) FROM auth_identities WHERE provider = 'password'")
            .await,
        0
    );
    assert_eq!(
        app.scalar("SELECT count(*) FROM users WHERE email_verified_at IS NOT NULL")
            .await,
        1
    );
    assert_eq!(
        app.scalar("SELECT count(*) FROM oauth_states").await,
        0,
        "state is single use"
    );
}

#[tokio::test]
async fn callback_requires_the_matching_state_cookie_and_a_fresh_state() {
    let app = App::new().await;
    app.account("code-1", "g-1", "ada@gmail.test", true);
    let (_, state) = app.start().await;

    for cookie in [None, Some("other-state")] {
        let response = app.callback("code-1", &state, cookie).await;
        assert_eq!(location(&response), "https://app.test/login?error=google");
        assert!(session_cookie(&response).is_none());
    }

    app.clock.advance(STATE_TTL);
    let response = app.callback("code-1", &state, Some(&state)).await;
    assert_eq!(location(&response), "https://app.test/login?error=google");
    assert!(
        app.fake.requests.lock().unwrap().is_empty(),
        "no exchange without a valid state"
    );
}

#[tokio::test]
async fn cancelled_consent_or_bad_code_lands_on_the_login_page() {
    let app = App::new().await;
    let (_, state) = app.start().await;
    let response = app
        .get("/auth/google/callback?error=access_denied", None)
        .await;
    assert_eq!(location(&response), "https://app.test/login?error=google");

    let response = app.callback("unknown-code", &state, Some(&state)).await;
    assert_eq!(location(&response), "https://app.test/login?error=google");
    assert_eq!(app.scalar("SELECT count(*) FROM users").await, 0);
}

#[tokio::test]
async fn native_flow_returns_a_bearer_token_and_reuses_the_account() {
    let app = App::new().await;
    app.account("n-1", "g-7", "nat@gmail.test", true);
    app.account("n-2", "g-7", "nat@gmail.test", true);

    let response = app.native("n-1", "ios-id").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["user"]["email_verified"], true);
    assert_eq!(body["user"]["display_name"], "Ada Google");
    assert_eq!(body["user"]["avatar_url"], "https://img.test/a.png");
    let token = body["token"].as_str().unwrap();
    let request = app.fake.requests.lock().unwrap()[0].clone();
    assert_eq!(request["client_id"], "ios-id");
    assert!(
        !request.contains_key("client_secret"),
        "native exchange has no secret"
    );

    assert_eq!(app.native("n-2", "ios-id").await.status(), StatusCode::OK);
    assert_eq!(app.scalar("SELECT count(*) FROM users").await, 1);
    let sessions = json_body(app.get_bearer("/auth/sessions", token).await).await;
    assert_eq!(sessions.as_array().unwrap().len(), 2);
}

impl App {
    async fn get_bearer(&self, uri: &str, token: &str) -> Response {
        let request = Request::builder()
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        self.router.clone().oneshot(request).await.unwrap()
    }
}

#[tokio::test]
async fn native_rejects_unknown_clients_bad_codes_and_bad_input() {
    let app = App::new().await;
    app.account("n-1", "g-1", "a@gmail.test", true);
    assert_eq!(
        app.native("n-1", "android-unknown").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.native("nope", "ios-id").await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .post(
            "/auth/google/native",
            None,
            json!({ "code": "", "code_verifier": "short", "redirect_uri": "x", "client_id": "ios-id" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn unverified_google_address_cannot_create_or_link_an_account() {
    let app = App::new().await;
    app.account("n-1", "g-1", "unverified@gmail.test", false);
    assert_eq!(
        app.native("n-1", "ios-id").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.scalar("SELECT count(*) FROM users").await, 0);
}

async fn register(app: &App, email: &str) -> String {
    let response = app
        .post(
            "/auth/register",
            None,
            json!({ "email": email, "password": "correct horse", "display_name": "Ada", "client": "native", "accepted_legal": identity::legal::current_acceptances() }),
        )
        .await;
    json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn password_login(app: &App, email: &str) -> StatusCode {
    app.post(
        "/auth/login",
        None,
        json!({ "email": email, "password": "correct horse", "client": "native" }),
    )
    .await
    .status()
}

#[tokio::test]
async fn google_links_to_a_verified_password_account() {
    let app = App::new().await;
    register(&app, "Ada@Gmail.test").await;
    sqlx::query("UPDATE users SET email_verified_at = now()")
        .execute(app.db.pool())
        .await
        .unwrap();
    app.account("n-1", "g-1", "ada@gmail.test", true);

    assert_eq!(app.native("n-1", "ios-id").await.status(), StatusCode::OK);
    assert_eq!(app.scalar("SELECT count(*) FROM users").await, 1);
    assert_eq!(app.scalar("SELECT count(*) FROM auth_identities").await, 2);
    assert_eq!(password_login(&app, "ada@gmail.test").await, StatusCode::OK);
}

#[tokio::test]
async fn linking_an_unverified_account_removes_the_squatters_password_and_sessions() {
    let app = App::new().await;
    let squatter = register(&app, "victim@gmail.test").await;
    app.account("n-1", "g-1", "victim@gmail.test", true);

    assert_eq!(app.native("n-1", "ios-id").await.status(), StatusCode::OK);
    assert_eq!(
        password_login(&app, "victim@gmail.test").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.get_bearer("/auth/sessions", &squatter).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.scalar("SELECT count(*) FROM users WHERE email_verified_at IS NOT NULL")
            .await,
        1
    );
}

#[tokio::test]
async fn an_account_links_only_one_google_account() {
    let app = App::new().await;
    app.account("n-1", "g-1", "one@gmail.test", true);
    assert_eq!(app.native("n-1", "ios-id").await.status(), StatusCode::OK);

    // A different Google account claiming the same address.
    app.account("n-2", "g-2", "one@gmail.test", true);
    let response = app.native("n-2", "ios-id").await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn expired_id_tokens_are_refused() {
    let app = App::new().await;
    app.account("n-1", "g-1", "late@gmail.test", true);
    app.clock.advance(Duration::hours(2));
    assert_eq!(
        app.native("n-1", "ios-id").await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn endpoints_are_absent_when_google_is_not_configured() {
    let app = App::build(false).await;
    assert!(!app.service.google_enabled());
    assert_eq!(
        app.get("/auth/google/start", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/auth/google/callback?code=a&state=b", None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.native("n-1", "ios-id").await.status(),
        StatusCode::NOT_FOUND
    );
}
