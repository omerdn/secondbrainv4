//! Auth endpoints and the [`Authenticated`] extractor (T0.3.2, ARCHITECTURE §15).
//!
//! | Method | Path                   | Auth | Result |
//! |--------|------------------------|------|--------|
//! | POST   | `/auth/register`       | –    | 201 [`AuthResponse`] |
//! | POST   | `/auth/login`          | –    | 200 [`AuthResponse`] |
//! | POST   | `/auth/logout`         | opt. | 204, ends the presented session |
//! | GET    | `/auth/csrf`           | yes  | 200 [`CsrfResponse`] for the cookie session |
//! | GET    | `/auth/sessions`       | yes  | 200 `SessionInfo[]` |
//! | DELETE | `/auth/sessions/{id}`  | yes  | 204 |
//! | POST   | `/auth/email/verify`   | –    | 204, or 422 on `token` |
//! | POST   | `/auth/email/verification` | yes | 204, sends a new verification e-mail |
//! | POST   | `/auth/password/forgot` | –   | 202 always (no account discovery) |
//! | POST   | `/auth/password/reset` | –    | 204, signs out everywhere; 422 on `token` |
//! | GET    | `/auth/google/start`   | –    | 303 to Google (web) |
//! | GET    | `/auth/google/callback` | –   | 303 to the web app, session cookie set |
//! | POST   | `/auth/google/native`  | –    | 200 [`AuthResponse`] with bearer token |
//! | GET    | `/auth/apple/start`    | –    | 303 to Apple (web) |
//! | POST   | `/auth/apple/callback` | –    | Apple's form POST; 303 to the web app |
//! | POST   | `/auth/apple/native`   | –    | 200 [`AuthResponse`] with bearer token |
//!
//! Google / Apple endpoints answer 404 when that provider is not configured.
//!
//! # Token transport
//! - **Web** (`client: "web"`): the token goes into an `HttpOnly; Secure; SameSite=Lax` cookie
//!   and is never in a response body, so page scripts cannot read it.
//! - **Native** (`client: "native"`): the token is returned once in the body; the app keeps it
//!   in secure storage and sends `Authorization: Bearer <token>`.
//!
//! # CSRF (T0.3.6)
//! A browser sends cookies automatically, so a cookie-authenticated `POST/PUT/PATCH/DELETE`
//! must also carry `X-CSRF-Token`, which a foreign site cannot know. The value is derived from
//! the session token ([`token::csrf_for`]); web clients get it in [`AuthResponse::csrf_token`]
//! and, after a page reload, from `GET /auth/csrf`. A missing or wrong value is 403. Bearer
//! requests need no CSRF token: browsers never attach an `Authorization` header on their own.
//! Unauthenticated JSON endpoints (login, register…) are covered by requiring
//! `Content-Type: application/json`, which cross-site HTML forms cannot send.
//!
//! A request with an `Authorization` header is authenticated by that header only (no cookie
//! fallback), so an invalid bearer token is always a 401.

use axum::extract::{FromRef, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use kernel::AppError;
use uuid::Uuid;

use crate::model::{
    AppleNativeRequest, AuthResponse, ClientKind, CurrentUser, ForgotPasswordRequest,
    GoogleNativeRequest, LoginRequest, RegisterRequest, ResetPasswordRequest, SessionId,
    SessionInfo, VerifyEmailRequest,
};
use crate::rate_limit::{self, RateLimits};
use crate::service::{AuthContext, IdentityService, IssuedSession};
use crate::token;

/// Name of the session cookie for web clients.
pub const SESSION_COOKIE: &str = "sb_session";

/// Cookie lifetime. The server-side expiry (sliding, see `SESSION_TTL`) decides validity;
/// the cookie only has to outlive it. 400 days is the maximum browsers honour.
const COOKIE_MAX_AGE_SECS: u64 = 400 * 24 * 60 * 60;

/// Auth routes with their state applied, **without** rate limits (tests, tools). The application
/// serves [`rate_limited_router`].
pub fn router(service: IdentityService) -> Router {
    routes(service, None)
}

/// [`router`] with per-client rate limits on the sensitive endpoints (T0.3.6). This is what
/// the application serves; the server must provide `ConnectInfo<SocketAddr>` (see
/// `app::server`) unless `trust_proxy_headers` is set.
pub fn rate_limited_router(service: IdentityService, limits: RateLimits) -> Router {
    routes(service, Some(limits))
}

fn routes(service: IdentityService, limits: Option<RateLimits>) -> Router {
    let credentials = Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/password/reset", post(reset_password))
        .route("/auth/email/verify", post(verify_email))
        .route("/auth/google/native", post(google_native))
        .route("/auth/apple/native", post(apple_native));
    let email = Router::new()
        .route("/auth/password/forgot", post(forgot_password))
        .route("/auth/email/verification", post(resend_verification));
    let (credentials, email) = match limits {
        Some(limits) => (
            rate_limit::limit(credentials, limits.credentials, limits),
            rate_limit::limit(email, limits.email, limits),
        ),
        None => (credentials, email),
    };
    Router::new()
        .route("/auth/logout", post(logout))
        .route("/auth/csrf", get(csrf))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{id}", delete(revoke_session))
        .route("/auth/google/start", get(google_start))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/apple/start", get(apple_start))
        .route("/auth/apple/callback", post(apple_callback))
        .merge(credentials)
        .merge(email)
        .with_state(service)
}

/// Extractor for endpoints that require a signed-in user. Rejects with 401
/// ([`AppError::Unauthenticated`]) when the token is missing, malformed, unknown or expired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Authenticated(pub AuthContext);

impl<S> FromRequestParts<S> for Authenticated
where
    IdentityService: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // A middleware earlier in the stack (e.g. idempotency) may have authenticated the
        // request already; extensions can only be set server-side.
        if let Some(auth) = parts.extensions.get::<AuthContext>() {
            return Ok(Self(*auth));
        }
        let service = IdentityService::from_ref(state);
        let auth = authenticate_request(&service, &parts.headers, &parts.method).await?;
        parts.extensions.insert(auth);
        Ok(Self(auth))
    }
}

/// Request header carrying the CSRF token.
pub const CSRF_HEADER: &str = "x-csrf-token";

fn is_unsafe(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Authenticates a request from its headers: bearer token, or session cookie plus — for
/// unsafe methods — a matching `X-CSRF-Token`.
///
/// Errors: 401 without a valid session; 403 when a cookie-authenticated mutation lacks the
/// right CSRF token.
pub async fn authenticate_request(
    service: &IdentityService,
    headers: &HeaderMap,
    method: &Method,
) -> Result<AuthContext, AppError> {
    let via_cookie = !headers.contains_key(header::AUTHORIZATION);
    let token = presented_token(headers).ok_or(AppError::Unauthenticated)?;
    let auth = service.authenticate(&token).await?;
    if via_cookie && is_unsafe(method) {
        let presented = headers
            .get(CSRF_HEADER)
            .map(HeaderValue::as_bytes)
            .unwrap_or_default();
        if !token::constant_time_eq(presented, token::csrf_for(&token).as_bytes()) {
            return Err(AppError::Forbidden);
        }
    }
    Ok(auth)
}

/// The token from `Authorization: Bearer` or, if there is no `Authorization` header, from the
/// session cookie.
pub fn presented_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let value = value.to_str().ok()?;
        let (scheme, token) = value.split_once(' ')?;
        return scheme
            .eq_ignore_ascii_case("bearer")
            .then(|| token.trim().to_owned());
    }
    cookie(headers, SESSION_COOKIE)
}

/// Value of the cookie `name`, if present.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}

/// Cookie holding the OAuth `state` between the redirect to Google and the callback.
const OAUTH_STATE_COOKIE: &str = "sb_oauth_state";

fn session_cookie(token: &str) -> HeaderValue {
    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}; HttpOnly; Secure; SameSite=Lax"
    );
    // The token is base64url, so the value is always a valid header.
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("sb_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax")
}

fn signed_in(
    status: StatusCode,
    client: ClientKind,
    user: CurrentUser,
    session: IssuedSession,
) -> Response {
    let IssuedSession { token, expires_at } = session;
    let (body_token, csrf_token, cookie) = match client {
        ClientKind::Native => (Some(token.expose().to_owned()), None, None),
        ClientKind::Web => (
            None,
            Some(token::csrf_for(token.expose())),
            Some(session_cookie(token.expose())),
        ),
    };
    let body = AuthResponse {
        user,
        token: body_token,
        csrf_token,
        expires_at,
    };
    let mut response = (status, Json(body)).into_response();
    if let Some(cookie) = cookie {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    // Never let a proxy or browser cache a response carrying a credential.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn register(
    State(service): State<IdentityService>,
    Json(request): Json<RegisterRequest>,
) -> Result<Response, AppError> {
    let client = request.client;
    let account = request.validate()?;
    let (user, session) = service.register(account).await?;
    Ok(signed_in(StatusCode::CREATED, client, user, session))
}

async fn login(
    State(service): State<IdentityService>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let client = request.client;
    let credentials = request.validate()?;
    let (user, session) = service.login(credentials).await?;
    Ok(signed_in(StatusCode::OK, client, user, session))
}

/// Ends the presented session if it is valid, and always clears the cookie, so signing out
/// works even with an already expired session.
async fn logout(State(service): State<IdentityService>, headers: HeaderMap) -> Response {
    // Without a valid CSRF token a cookie session is not revoked (only the cookie is cleared).
    if let Ok(auth) = authenticate_request(&service, &headers, &Method::POST).await {
        match service.revoke_session(auth.user_id, auth.session_id).await {
            Ok(()) | Err(AppError::NotFound) => {}
            Err(err) => return err.into_response(),
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_cookie());
    response
}

/// Body of `GET /auth/csrf`.
#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct CsrfResponse {
    /// Value for the `X-CSRF-Token` header of cookie-authenticated mutations.
    pub csrf_token: String,
}

/// The CSRF token of the presented session (for a web app after a page reload).
async fn csrf(
    State(service): State<IdentityService>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let auth_token = presented_token(&headers).ok_or(AppError::Unauthenticated)?;
    service.authenticate(&auth_token).await?;
    let mut response = Json(CsrfResponse {
        csrf_token: token::csrf_for(&auth_token),
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn list_sessions(
    State(service): State<IdentityService>,
    Authenticated(auth): Authenticated,
) -> Result<Json<Vec<SessionInfo>>, AppError> {
    service.list_sessions(auth).await.map(Json)
}

async fn revoke_session(
    State(service): State<IdentityService>,
    Authenticated(auth): Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // An unparsable id cannot name one of the caller's sessions: same answer as a foreign one.
    let id = id.parse::<Uuid>().map_err(|_| AppError::NotFound)?;
    service
        .revoke_session(auth.user_id, SessionId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn verify_email(
    State(service): State<IdentityService>,
    Json(request): Json<VerifyEmailRequest>,
) -> Result<StatusCode, AppError> {
    service.verify_email(&request.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resend_verification(
    State(service): State<IdentityService>,
    Authenticated(auth): Authenticated,
) -> Result<StatusCode, AppError> {
    service.resend_verification(auth).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn forgot_password(
    State(service): State<IdentityService>,
    Json(request): Json<ForgotPasswordRequest>,
) -> Result<StatusCode, AppError> {
    let email = request.validate()?;
    service.forgot_password(&email).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn reset_password(
    State(service): State<IdentityService>,
    Json(request): Json<ResetPasswordRequest>,
) -> Result<Response, AppError> {
    let (token, password) = request.validate()?;
    service.reset_password(&token, password).await?;
    // The browser's session (if any) has just been revoked too.
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_cookie());
    Ok(response)
}

/// Browser sign-in kind: decides the state cookie's path and `SameSite` and the error tag.
#[derive(Clone, Copy)]
enum WebProvider {
    Google,
    Apple,
}

impl WebProvider {
    fn path(self) -> &'static str {
        match self {
            Self::Google => "/auth/google",
            Self::Apple => "/auth/apple",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
        }
    }

    /// Google returns with a top-level GET (Lax cookies are sent); Apple returns with a
    /// cross-site form POST, which only carries `SameSite=None` cookies.
    fn same_site(self) -> &'static str {
        match self {
            Self::Google => "Lax",
            Self::Apple => "None",
        }
    }

    fn state_cookie(self, value: &str, max_age: i64) -> Result<HeaderValue, AppError> {
        HeaderValue::from_str(&format!(
            "{OAUTH_STATE_COOKIE}={value}; Path={}; Max-Age={max_age}; HttpOnly; Secure; SameSite={}",
            self.path(),
            self.same_site()
        ))
        .map_err(AppError::internal)
    }
}

/// 303 to the provider with the state cookie set.
fn redirect_to_provider(
    provider: WebProvider,
    url: &str,
    state: &str,
    max_age: i64,
) -> Result<Response, AppError> {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(url).map_err(AppError::internal)?,
    );
    headers.insert(header::SET_COOKIE, provider.state_cookie(state, max_age)?);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Ends a browser sign-in with a redirect to the web app either way: `/` (session cookie set)
/// on success, `/login?error=<provider>` on failure (cancelled consent, expired state, refused
/// account…). The state cookie is cleared.
fn finish_web_sign_in(
    provider: WebProvider,
    web_url: &str,
    result: Result<(CurrentUser, IssuedSession), AppError>,
) -> Response {
    let (location, session) = match result {
        Ok((_, session)) => (
            format!("{web_url}/"),
            Some(session_cookie(session.token.expose())),
        ),
        Err(err) => {
            if let AppError::Internal(_) = err {
                // Logged with the request context; the user still lands on the login page.
                let _ = err.into_response();
            }
            (format!("{web_url}/login?error={}", provider.name()), None)
        }
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    let out = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&location) {
        out.insert(header::LOCATION, value);
    }
    if let Ok(clear) = provider.state_cookie("", 0) {
        out.append(header::SET_COOKIE, clear);
    }
    if let Some(cookie) = session {
        out.append(header::SET_COOKIE, cookie);
    }
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn google_start(State(service): State<IdentityService>) -> Result<Response, AppError> {
    let start = service.google_web_start().await?;
    redirect_to_provider(
        WebProvider::Google,
        &start.authorization_url,
        &start.state,
        crate::oauth::google::STATE_TTL.num_seconds(),
    )
}

/// Query of Google's redirect back to us.
#[derive(Debug, serde::Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn google_callback(
    State(service): State<IdentityService>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<CallbackQuery>,
) -> Response {
    let Some(web_url) = service.google_web_url() else {
        return AppError::NotFound.into_response();
    };
    let cookie_state = cookie(&headers, OAUTH_STATE_COOKIE);
    let result = match (query.code, query.state) {
        (Some(code), Some(state)) => {
            service
                .google_web_callback(code, &state, cookie_state.as_deref())
                .await
        }
        _ => Err(AppError::Unauthenticated),
    };
    finish_web_sign_in(WebProvider::Google, &web_url, result)
}

async fn apple_start(State(service): State<IdentityService>) -> Result<Response, AppError> {
    let start = service.apple_web_start().await?;
    redirect_to_provider(
        WebProvider::Apple,
        &start.authorization_url,
        &start.state,
        crate::oauth::apple::STATE_TTL.num_seconds(),
    )
}

/// Apple's `form_post` back to us. `user` is only present on the first authorization.
#[derive(Debug, serde::Deserialize)]
struct AppleCallbackForm {
    code: Option<String>,
    state: Option<String>,
    user: Option<String>,
}

async fn apple_callback(
    State(service): State<IdentityService>,
    headers: HeaderMap,
    axum::extract::Form(form): axum::extract::Form<AppleCallbackForm>,
) -> Response {
    let Some(web_url) = service.apple_web_url() else {
        return AppError::NotFound.into_response();
    };
    let cookie_state = cookie(&headers, OAUTH_STATE_COOKIE);
    let result = match (form.code, form.state) {
        (Some(code), Some(state)) => {
            service
                .apple_web_callback(code, &state, cookie_state.as_deref(), form.user.as_deref())
                .await
        }
        _ => Err(AppError::Unauthenticated),
    };
    finish_web_sign_in(WebProvider::Apple, &web_url, result)
}

async fn apple_native(
    State(service): State<IdentityService>,
    Json(request): Json<AppleNativeRequest>,
) -> Result<Response, AppError> {
    let sign_in = request.validate()?;
    let (user, session) = service.apple_native(sign_in).await?;
    Ok(signed_in(StatusCode::OK, ClientKind::Native, user, session))
}

async fn google_native(
    State(service): State<IdentityService>,
    Json(request): Json<GoogleNativeRequest>,
) -> Result<Response, AppError> {
    let sign_in = request.validate()?;
    let (user, session) = service.google_native(sign_in).await?;
    Ok(signed_in(StatusCode::OK, ClientKind::Native, user, session))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(name.clone(), HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn bearer_token_is_read_case_insensitively() {
        let h = headers(&[(header::AUTHORIZATION, "bearer abc")]);
        assert_eq!(presented_token(&h).as_deref(), Some("abc"));
    }

    #[test]
    fn authorization_header_wins_over_cookie_without_fallback() {
        let h = headers(&[
            (header::AUTHORIZATION, "Basic xyz"),
            (header::COOKIE, "sb_session=fromcookie"),
        ]);
        assert_eq!(presented_token(&h), None);
    }

    #[test]
    fn session_cookie_is_found_among_others() {
        let h = headers(&[
            (header::COOKIE, "theme=dark"),
            (header::COOKIE, "a=1; sb_session=tok; b=2"),
        ]);
        assert_eq!(presented_token(&h).as_deref(), Some("tok"));
        assert_eq!(
            presented_token(&headers(&[(header::COOKIE, "xsb_session=no")])),
            None
        );
    }

    #[test]
    fn cookie_attributes_follow_architecture_15() {
        let cookie = session_cookie("tok");
        let cookie = cookie.to_str().unwrap();
        for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
            assert!(cookie.contains(attr), "{cookie}");
        }
    }
}
