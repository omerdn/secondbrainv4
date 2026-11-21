//! Auth endpoints and the [`Authenticated`] extractor (T0.3.2, ARCHITECTURE §15).
//!
//! | Method | Path                   | Auth | Result |
//! |--------|------------------------|------|--------|
//! | POST   | `/auth/register`       | –    | 201 [`AuthResponse`] |
//! | POST   | `/auth/login`          | –    | 200 [`AuthResponse`] |
//! | POST   | `/auth/logout`         | opt. | 204, ends the presented session |
//! | GET    | `/auth/sessions`       | yes  | 200 `SessionInfo[]` |
//! | DELETE | `/auth/sessions/{id}`  | yes  | 204 |
//! | POST   | `/auth/email/verify`   | –    | 204, or 422 on `token` |
//! | POST   | `/auth/email/verification` | yes | 204, sends a new verification e-mail |
//! | POST   | `/auth/password/forgot` | –   | 202 always (no account discovery) |
//! | POST   | `/auth/password/reset` | –    | 204, signs out everywhere; 422 on `token` |
//! | GET    | `/auth/google/start`   | –    | 303 to Google (web) |
//! | GET    | `/auth/google/callback` | –   | 303 to the web app, session cookie set |
//! | POST   | `/auth/google/native`  | –    | 200 [`AuthResponse`] with bearer token |
//!
//! Google endpoints answer 404 when Google sign-in is not configured.
//!
//! # Token transport
//! - **Web** (`client: "web"`): the token goes into an `HttpOnly; Secure; SameSite=Lax` cookie
//!   and is never in a response body, so page scripts cannot read it. CSRF protection for
//!   cookie-authenticated mutations is added in T0.3.6.
//! - **Native** (`client: "native"`): the token is returned once in the body; the app keeps it
//!   in secure storage and sends `Authorization: Bearer <token>`.
//!
//! A request with an `Authorization` header is authenticated by that header only (no cookie
//! fallback), so an invalid bearer token is always a 401.

use axum::extract::{FromRef, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use kernel::AppError;
use uuid::Uuid;

use crate::model::{
    AuthResponse, ClientKind, CurrentUser, ForgotPasswordRequest, GoogleNativeRequest,
    LoginRequest, RegisterRequest, ResetPasswordRequest, SessionId, SessionInfo,
    VerifyEmailRequest,
};
use crate::service::{AuthContext, IdentityService, IssuedSession};

/// Name of the session cookie for web clients.
pub const SESSION_COOKIE: &str = "sb_session";

/// Cookie lifetime. The server-side expiry (sliding, see `SESSION_TTL`) decides validity;
/// the cookie only has to outlive it. 400 days is the maximum browsers honour.
const COOKIE_MAX_AGE_SECS: u64 = 400 * 24 * 60 * 60;

/// Auth routes with their state applied, ready to merge into the application router.
pub fn router(service: IdentityService) -> Router {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{id}", delete(revoke_session))
        .route("/auth/email/verify", post(verify_email))
        .route("/auth/email/verification", post(resend_verification))
        .route("/auth/password/forgot", post(forgot_password))
        .route("/auth/password/reset", post(reset_password))
        .route("/auth/google/start", get(google_start))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/google/native", post(google_native))
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
        let token = presented_token(&parts.headers).ok_or(AppError::Unauthenticated)?;
        let service = IdentityService::from_ref(state);
        let auth = service.authenticate(&token).await?;
        parts.extensions.insert(auth);
        Ok(Self(auth))
    }
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
    let (body_token, cookie) = match client {
        ClientKind::Native => (Some(token.expose().to_owned()), None),
        ClientKind::Web => (None, Some(session_cookie(token.expose()))),
    };
    let body = AuthResponse {
        user,
        token: body_token,
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
    if let Some(token) = presented_token(&headers)
        && let Ok(auth) = service.authenticate(&token).await
    {
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

async fn google_start(State(service): State<IdentityService>) -> Result<Response, AppError> {
    let start = service.google_web_start().await?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&start.authorization_url).map_err(AppError::internal)?,
    );
    let max_age = crate::oauth::google::STATE_TTL.num_seconds();
    let cookie = format!(
        "{OAUTH_STATE_COOKIE}={}; Path=/auth/google; Max-Age={max_age}; HttpOnly; Secure; SameSite=Lax",
        start.state
    );
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(AppError::internal)?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Query of Google's redirect back to us.
#[derive(Debug, serde::Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// Ends with a redirect to the web app either way: `/` on success, `/login?error=google` on
/// failure (cancelled consent, expired state, refused account…).
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
    let (location, session_cookie) = match result {
        Ok((_, session)) => (
            format!("{web_url}/"),
            Some(session_cookie(session.token.expose())),
        ),
        Err(AppError::Internal(err)) => {
            // Logged with the request context; the user still lands on the login page.
            let _ = AppError::Internal(err).into_response();
            (format!("{web_url}/login?error=google"), None)
        }
        Err(_) => (format!("{web_url}/login?error=google"), None),
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    let out = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&location) {
        out.insert(header::LOCATION, value);
    }
    out.append(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "sb_oauth_state=; Path=/auth/google; Max-Age=0; HttpOnly; Secure; SameSite=Lax",
        ),
    );
    if let Some(cookie) = session_cookie {
        out.append(header::SET_COOKIE, cookie);
    }
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
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
