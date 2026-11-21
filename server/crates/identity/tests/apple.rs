//! Sign in with Apple (T0.3.5) against a real Postgres 17 and a fake Apple token endpoint.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Form, State};
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use http_body_util::BodyExt;
use identity::oauth::apple::{Apple, AppleConfig};
use identity::{EmailSettings, IdentityService};
use kernel::email::RecordingEmailSender;
use kernel::{Clock, FixedClock};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{SigningKey, VerifyingKey};
use p256::elliptic_curve::Generate;
use p256::pkcs8::EncodePrivateKey;
use serde_json::{Value, json};
use tower::ServiceExt;

/// An Apple account the fake provider knows, keyed by authorization code.
#[derive(Clone)]
struct Account {
    sub: &'static str,
    /// `None` models Apple omitting the address.
    email: Option<&'static str>,
}

#[derive(Clone)]
struct FakeApple {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
    requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    verifying_key: VerifyingKey,
    exp: i64,
}

/// Accepts the code only with a client secret signed by the team key for the same client id.
async fn token_endpoint(
    State(fake): State<FakeApple>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    fake.requests.lock().unwrap().push(form.clone());
    let client_id = form.get("client_id").cloned().unwrap_or_default();
    let secret = form.get("client_secret").cloned().unwrap_or_default();
    let parts: Vec<&str> = secret.split('.').collect();
    let signature_ok = parts.len() == 3
        && URL_SAFE_NO_PAD
            .decode(parts[2])
            .ok()
            .and_then(|sig| p256::ecdsa::Signature::from_slice(&sig).ok())
            .is_some_and(|sig| {
                fake.verifying_key
                    .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
                    .is_ok()
            });
    let claims: Value = parts
        .get(1)
        .and_then(|p| URL_SAFE_NO_PAD.decode(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    if !signature_ok || claims["sub"] != client_id.as_str() || claims["iss"] != "TEAM123" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_client" })),
        )
            .into_response();
    }
    let code = form.get("code").cloned().unwrap_or_default();
    let Some(account) = fake.accounts.lock().unwrap().get(&code).cloned() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant" })),
        )
            .into_response();
    };
    let mut id = json!({
        "iss": "https://appleid.apple.com", "aud": client_id, "sub": account.sub, "exp": fake.exp,
    });
    if let Some(email) = account.email {
        id["email"] = json!(email);
        id["email_verified"] = json!("true");
        id["is_private_email"] = json!(email.ends_with("privaterelay.appleid.com").to_string());
    }
    let id_token = format!(
        "{}.{}.sig",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256"}"#),
        URL_SAFE_NO_PAD.encode(id.to_string())
    );
    Json(json!({ "access_token": "at", "token_type": "Bearer", "expires_in": 3600, "id_token": id_token }))
        .into_response()
}

struct App {
    db: TestDb,
    fake: FakeApple,
    router: Router,
    service: IdentityService,
}

impl App {
    async fn new() -> Self {
        Self::build(true).await
    }

    async fn build(with_apple: bool) -> Self {
        let db = TestDb::new().await;
        let clock = FixedClock::new(Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap());
        let key = SigningKey::generate();
        let fake = FakeApple {
            accounts: Arc::default(),
            requests: Arc::default(),
            verifying_key: VerifyingKey::from(&key),
            exp: clock.now().timestamp() + 600,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake_router = Router::new()
            .route("/token", post(token_endpoint))
            .with_state(fake.clone());
        tokio::spawn(async move { axum::serve(listener, fake_router).await.unwrap() });

        let mut service = IdentityService::new(
            db.pool().clone(),
            Arc::new(clock),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        if with_apple {
            service = service.with_apple(
                Apple::new(AppleConfig {
                    team_id: "TEAM123".into(),
                    key_id: "KEY123".into(),
                    private_key_pem: key.to_pkcs8_pem(Default::default()).unwrap().to_string(),
                    services_id: "com.example.web".into(),
                    bundle_ids: vec!["com.example.app".into()],
                    callback_url: "https://api.test/auth/apple/callback".into(),
                    web_url: "https://app.test".into(),
                    auth_url: "https://appleid.test/auth/authorize".into(),
                    token_url: format!("http://{addr}/token"),
                })
                .unwrap(),
            );
        }
        let router = identity::http::router(service.clone());
        Self {
            db,
            fake,
            router,
            service,
        }
    }

    fn account(&self, code: &str, sub: &'static str, email: Option<&'static str>) {
        self.fake
            .accounts
            .lock()
            .unwrap()
            .insert(code.into(), Account { sub, email });
    }

    async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    async fn native(&self, body: Value) -> Response {
        self.send(
            Request::post("/auth/apple/native")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    async fn callback(&self, form: &str, cookie_state: Option<&str>) -> Response {
        let mut request = Request::post("/auth/apple/callback")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(state) = cookie_state {
            request = request.header(header::COOKIE, format!("sb_oauth_state={state}"));
        }
        self.send(request.body(Body::from(form.to_owned())).unwrap())
            .await
    }

    async fn users(&self) -> Vec<(String, String)> {
        sqlx::query_as("SELECT email, display_name FROM users ORDER BY created_at, id")
            .fetch_all(self.db.pool())
            .await
            .unwrap()
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn header_values(response: &Response, name: header::HeaderName) -> Vec<String> {
    response
        .headers()
        .get_all(name)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect()
}

fn param(url: &str, name: &str) -> String {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix(&format!("{name}=")))
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn web_flow_uses_form_post_and_takes_the_name_from_the_first_authorization() {
    let app = App::new().await;
    app.account("c-1", "001.web", Some("ada@icloud.test"));

    let start = app
        .send(
            Request::get("/auth/apple/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(start.status(), StatusCode::SEE_OTHER);
    let url = header_values(&start, header::LOCATION).remove(0);
    assert!(url.starts_with("https://appleid.test/auth/authorize?"));
    assert_eq!(param(&url, "response_mode"), "form_post");
    assert_eq!(param(&url, "client_id"), "com.example.web");
    assert_eq!(param(&url, "scope"), "name+email");
    let cookie = header_values(&start, header::SET_COOKIE).remove(0);
    assert!(
        cookie.contains("SameSite=None") && cookie.contains("Secure"),
        "{cookie}"
    );
    let state = param(&url, "state");

    let user = r#"{"name":{"firstName":"Ada","lastName":"Lovelace"},"email":"ada@icloud.test"}"#;
    let form = format!("code=c-1&state={state}&user={}", urlencode(user));
    let response = app.callback(&form, Some(&state)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_values(&response, header::LOCATION),
        ["https://app.test/"]
    );
    assert!(
        header_values(&response, header::SET_COOKIE)
            .iter()
            .any(|c| c.starts_with("sb_session="))
    );

    let request = app.fake.requests.lock().unwrap()[0].clone();
    assert_eq!(request["client_id"], "com.example.web");
    assert_eq!(
        request["redirect_uri"],
        "https://api.test/auth/apple/callback"
    );
    assert_eq!(
        app.users().await,
        [("ada@icloud.test".to_owned(), "Ada Lovelace".to_owned())]
    );
    let provider: String = sqlx::query_scalar("SELECT provider FROM auth_identities")
        .fetch_one(app.db.pool())
        .await
        .unwrap();
    assert_eq!(provider, "apple");
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn web_callback_requires_the_state_cookie() {
    let app = App::new().await;
    app.account("c-1", "001.web", Some("ada@icloud.test"));
    let start = app
        .send(
            Request::get("/auth/apple/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let state = param(&header_values(&start, header::LOCATION)[0], "state");

    let response = app.callback(&format!("code=c-1&state={state}"), None).await;
    assert_eq!(
        header_values(&response, header::LOCATION),
        ["https://app.test/login?error=apple"]
    );
    assert!(app.fake.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_sign_in_keeps_the_first_name_and_works_without_an_address_later() {
    let app = App::new().await;
    app.account("n-1", "001.nat", Some("nat@icloud.test"));
    app.account("n-2", "001.nat", None);

    let first = app
        .native(json!({ "code": "n-1", "client_id": "com.example.app", "given_name": "Ada", "family_name": "L", "device": "iPhone" }))
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let body = json_body(first).await;
    assert_eq!(body["user"]["display_name"], "Ada L");
    assert!(body["token"].as_str().is_some());

    // Later sign-ins: no name, no address.
    let second = app
        .native(json!({ "code": "n-2", "client_id": "com.example.app" }))
        .await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(json_body(second).await["user"]["display_name"], "Ada L");
    assert_eq!(app.users().await.len(), 1);

    let request = app.fake.requests.lock().unwrap()[0].clone();
    assert_eq!(request["client_id"], "com.example.app");
    assert!(!request.contains_key("redirect_uri"));
}

#[tokio::test]
async fn private_relay_address_creates_a_separate_verified_account() {
    let app = App::new().await;
    // Someone already registered with their real address.
    sqlx::query(
        "INSERT INTO users (id, email, display_name, created_at, email_verified_at)
         VALUES ('0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b', 'ada@real.test', 'Ada', now(), now())",
    )
    .execute(app.db.pool())
    .await
    .unwrap();
    app.account("n-1", "001.relay", Some("x7k2@privaterelay.appleid.com"));

    let response = app
        .native(json!({ "code": "n-1", "client_id": "com.example.app" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["user"]["email"], "x7k2@privaterelay.appleid.com");
    assert_eq!(body["user"]["email_verified"], true);
    assert_eq!(
        body["user"]["display_name"], "x7k2",
        "no name sent: local part"
    );
    assert_eq!(app.users().await.len(), 2);
}

#[tokio::test]
async fn native_rejects_unknown_bundles_missing_address_and_bad_input() {
    let app = App::new().await;
    app.account("n-1", "001.a", Some("a@icloud.test"));
    app.account("n-2", "001.b", None);

    let unknown = app
        .native(json!({ "code": "n-1", "client_id": "com.evil.app" }))
        .await;
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    // First sign-in without an address cannot create an account.
    let no_email = app
        .native(json!({ "code": "n-2", "client_id": "com.example.app" }))
        .await;
    assert_eq!(no_email.status(), StatusCode::UNAUTHORIZED);
    let bad_code = app
        .native(json!({ "code": "nope", "client_id": "com.example.app" }))
        .await;
    assert_eq!(bad_code.status(), StatusCode::UNAUTHORIZED);
    let invalid = app
        .native(json!({ "code": "", "client_id": "com.example.app" }))
        .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(app.users().await.is_empty());
}

#[tokio::test]
async fn endpoints_are_absent_when_apple_is_not_configured() {
    let app = App::build(false).await;
    assert!(!app.service.apple_enabled());
    let start = app
        .send(
            Request::get("/auth/apple/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(start.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        app.callback("code=a&state=b", None).await.status(),
        StatusCode::NOT_FOUND
    );
    let native = app
        .native(json!({ "code": "a", "client_id": "com.example.app" }))
        .await;
    assert_eq!(native.status(), StatusCode::NOT_FOUND);
}
