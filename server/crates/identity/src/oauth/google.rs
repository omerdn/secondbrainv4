//! Google sign-in (T0.3.4) with the authorization code flow + PKCE (`oauth2` crate).
//!
//! - **Web:** `GET /auth/google/start` stores a one-time `state` (+ PKCE verifier) and redirects
//!   to Google; Google redirects back to `GET /auth/google/callback`, the server exchanges the
//!   code with the web client's secret and sets the session cookie. The `state` is also kept in a
//!   short-lived cookie and must match on callback, so an attacker cannot make a victim's browser
//!   finish the attacker's sign-in (login CSRF).
//! - **Native:** the app runs the flow itself with its platform client id (no secret) and sends
//!   the code, PKCE verifier and redirect URI to `POST /auth/google/native`; the server performs
//!   the exchange and returns a bearer token.
//!
//! The ID token comes straight from Google's token endpoint over TLS, so per OpenID Connect Core
//! §3.1.3.7 the TLS server validation replaces signature validation; `iss`, `aud` and `exp` are
//! still checked.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use kernel::AppError;
use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenType,
};
use oauth2::{
    AuthUrl, AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, ExtraTokenFields, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    StandardRevocableToken, StandardTokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};

use super::{Provider, ProviderClaims};
use crate::model::CurrentUser;
use crate::service::{IdentityService, IssuedSession};
use crate::token::{self, SessionToken};

/// Google's authorization endpoint.
pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
/// Google's token endpoint.
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// How long a browser has to come back from Google.
pub const STATE_TTL: Duration = Duration::minutes(10);
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// Google OAuth clients. Created in the Google Cloud console (one web client with a secret,
/// one client per native platform without a secret).
#[derive(Clone)]
pub struct GoogleConfig {
    /// Web client id.
    pub web_client_id: String,
    /// Web client secret.
    pub web_client_secret: String,
    /// iOS / Android client ids accepted from native apps.
    pub native_client_ids: Vec<String>,
    /// This server's callback, registered as redirect URI of the web client
    /// (e.g. `https://api.example.com/auth/google/callback`).
    pub callback_url: String,
    /// Where the browser goes after a web sign-in (the web app's URL).
    pub web_url: String,
    /// Authorization endpoint (overridable for tests).
    pub auth_url: String,
    /// Token endpoint (overridable for tests).
    pub token_url: String,
}

impl std::fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleConfig")
            .field("web_client_id", &self.web_client_id)
            .finish_non_exhaustive()
    }
}

/// Configured Google integration (config + HTTP client).
#[derive(Debug, Clone)]
pub struct Google {
    config: GoogleConfig,
    http: oauth2::reqwest::Client,
}

impl Google {
    /// Builds the integration. Redirects are disabled on the HTTP client (oauth2 crate advice:
    /// prevents SSRF through a malicious token endpoint redirect).
    pub fn new(config: GoogleConfig) -> Result<Self, AppError> {
        let http = oauth2::reqwest::Client::builder()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(AppError::internal)?;
        Ok(Self { config, http })
    }

    /// The web app URL to send the browser to after the callback.
    pub fn web_url(&self) -> &str {
        &self.config.web_url
    }
}

/// `id_token` from the token response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdTokenFields {
    id_token: String,
}
impl ExtraTokenFields for IdTokenFields {}

type GoogleClient = Client<
    BasicErrorResponse,
    StandardTokenResponse<IdTokenFields, BasicTokenType>,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

fn client(
    config: &GoogleConfig,
    client_id: &str,
    secret: Option<&str>,
    redirect: &str,
) -> Result<GoogleClient, AppError> {
    let mut client = Client::new(ClientId::new(client_id.to_owned()))
        .set_auth_uri(AuthUrl::new(config.auth_url.clone()).map_err(AppError::internal)?)
        .set_token_uri(TokenUrl::new(config.token_url.clone()).map_err(AppError::internal)?)
        .set_redirect_uri(
            RedirectUrl::new(redirect.to_owned()).map_err(|_| refused("bad redirect uri"))?,
        );
    if let Some(secret) = secret {
        client = client.set_client_secret(ClientSecret::new(secret.to_owned()));
    }
    Ok(client)
}

fn refused(reason: &'static str) -> AppError {
    tracing::info!(provider = "google", reason, "provider sign-in refused");
    AppError::Unauthenticated
}

/// Claims of a Google ID token that we use.
#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    aud: serde_json::Value,
    sub: String,
    exp: i64,
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<serde_json::Value>,
    name: Option<String>,
    picture: Option<String>,
}

/// Decodes and checks an ID token received directly from Google's token endpoint.
pub fn verify_id_token(
    id_token: &str,
    expected_audience: &str,
    now: DateTime<Utc>,
) -> Result<ProviderClaims, AppError> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| refused("malformed id token"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| refused("malformed id token"))?;
    let claims: IdTokenClaims =
        serde_json::from_slice(&bytes).map_err(|_| refused("malformed id token"))?;

    if !ISSUERS.contains(&claims.iss.as_str()) {
        return Err(refused("wrong issuer"));
    }
    let audience_ok = match &claims.aud {
        serde_json::Value::String(aud) => aud == expected_audience,
        serde_json::Value::Array(auds) => auds.iter().any(|a| a == expected_audience),
        _ => false,
    };
    if !audience_ok {
        return Err(refused("wrong audience"));
    }
    if claims.exp <= now.timestamp() {
        return Err(refused("id token expired"));
    }
    let email_verified = match claims.email_verified {
        Some(serde_json::Value::Bool(b)) => b,
        Some(serde_json::Value::String(s)) => s == "true",
        _ => false,
    };
    Ok(ProviderClaims {
        subject: claims.sub,
        email: claims.email,
        email_verified,
        name: claims.name,
        picture: claims.picture,
    })
}

impl Google {
    async fn exchange(
        &self,
        client: &GoogleClient,
        code: String,
        verifier: String,
    ) -> Result<String, AppError> {
        let response = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(&self.http)
            .await
            .map_err(|err| {
                // The error type carries no personal data (OAuth error codes / transport errors).
                tracing::info!(provider = "google", error = %err, "code exchange failed");
                AppError::Unauthenticated
            })?;
        Ok(response.extra_fields().id_token.clone())
    }
}

/// Result of starting a browser sign-in.
#[derive(Debug)]
pub struct WebStart {
    /// Google authorization URL to redirect the browser to.
    pub authorization_url: String,
    /// The `state` value, also set as a cookie.
    pub state: String,
}

impl IdentityService {
    fn google(&self) -> Result<&Google, AppError> {
        // Not configured: behave as if the endpoint did not exist.
        self.inner().google.as_ref().ok_or(AppError::NotFound)
    }

    /// The web app URL to return to after a browser sign-in, if Google is configured.
    pub fn google_web_url(&self) -> Option<String> {
        self.inner()
            .google
            .as_ref()
            .map(|g| g.web_url().trim_end_matches('/').to_owned())
    }

    /// Whether Google sign-in is configured.
    pub fn google_enabled(&self) -> bool {
        self.inner().google.is_some()
    }

    /// Starts a browser sign-in: stores state + PKCE verifier and returns the Google URL.
    pub async fn google_web_start(&self) -> Result<WebStart, AppError> {
        let google = self.google()?;
        let config = &google.config;
        let client = client(
            config,
            &config.web_client_id,
            Some(&config.web_client_secret),
            &config.callback_url,
        )?;
        let state = SessionToken::generate().map_err(AppError::internal)?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state_value = state.expose().to_owned();
        let (url, _) = client
            .authorize_url(move || CsrfToken::new(state_value))
            .add_scope(Scope::new("openid".into()))
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .add_extra_param("prompt", "select_account")
            .set_pkce_challenge(challenge)
            .url();

        let now = self.now();
        sqlx::query(
            "INSERT INTO oauth_states (state_hash, provider, pkce_verifier, created_at, expires_at)
             VALUES ($1, 'google', $2, $3, $4)",
        )
        .bind(state.hash().as_slice())
        .bind(verifier.secret())
        .bind(now)
        .bind(now + STATE_TTL)
        .execute(self.pool())
        .await
        .map_err(AppError::internal)?;
        Ok(WebStart {
            authorization_url: url.to_string(),
            state: state.expose().to_owned(),
        })
    }

    /// Finishes a browser sign-in. `cookie_state` is the state cookie set at start.
    pub async fn google_web_callback(
        &self,
        code: String,
        state: &str,
        cookie_state: Option<&str>,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let google = self.google()?;
        if cookie_state != Some(state) {
            return Err(refused("state cookie mismatch"));
        }
        let hash = token::hash_presented(state).ok_or_else(|| refused("malformed state"))?;
        let verifier = sqlx::query_scalar::<_, String>(
            "DELETE FROM oauth_states
             WHERE state_hash = $1 AND provider = 'google' AND expires_at > $2
             RETURNING pkce_verifier",
        )
        .bind(hash.as_slice())
        .bind(self.now())
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| refused("unknown or expired state"))?;

        let config = &google.config;
        let client = client(
            config,
            &config.web_client_id,
            Some(&config.web_client_secret),
            &config.callback_url,
        )?;
        let id_token = google.exchange(&client, code, verifier).await?;
        let claims = verify_id_token(&id_token, &config.web_client_id, self.now())?;
        self.sign_in_with_provider(Provider::Google, claims, Some("Web · Google"))
            .await
    }

    /// Finishes a native app sign-in (the app holds the PKCE verifier).
    pub async fn google_native(
        &self,
        request: GoogleNativeSignIn,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let google = self.google()?;
        let config = &google.config;
        if !config.native_client_ids.contains(&request.client_id) {
            return Err(refused("unknown native client id"));
        }
        let client = client(config, &request.client_id, None, &request.redirect_uri)?;
        let id_token = google
            .exchange(&client, request.code, request.code_verifier)
            .await?;
        let claims = verify_id_token(&id_token, &request.client_id, self.now())?;
        self.sign_in_with_provider(Provider::Google, claims, request.device.as_deref())
            .await
    }
}

/// Validated `POST /auth/google/native` input.
#[derive(Debug)]
pub struct GoogleNativeSignIn {
    pub code: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub client_id: String,
    pub device: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn jwt(claims: serde_json::Value) -> String {
        let enc = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
        format!("{}.{}.sig", enc(&json!({"alg": "RS256"})), enc(&claims))
    }

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000, 0).unwrap()
    }

    fn base() -> serde_json::Value {
        json!({
            "iss": "https://accounts.google.com", "aud": "web-id", "sub": "123",
            "exp": 1_800_000_060, "email": "ada@example.test", "email_verified": true,
            "name": "Ada", "picture": "https://img.test/a.png"
        })
    }

    #[test]
    fn valid_token_yields_claims() {
        let claims = verify_id_token(&jwt(base()), "web-id", now()).unwrap();
        assert_eq!(claims.subject, "123");
        assert_eq!(claims.email.as_deref(), Some("ada@example.test"));
        assert!(claims.email_verified);
        assert_eq!(claims.name.as_deref(), Some("Ada"));
    }

    #[test]
    fn issuer_audience_and_expiry_are_checked() {
        let mut t = base();
        t["iss"] = json!("https://evil.test");
        assert!(verify_id_token(&jwt(t), "web-id", now()).is_err());
        assert!(verify_id_token(&jwt(base()), "other-id", now()).is_err());
        let mut t = base();
        t["exp"] = json!(1_800_000_000);
        assert!(verify_id_token(&jwt(t), "web-id", now()).is_err());
        let mut t = base();
        t["aud"] = json!(["x", "web-id"]);
        assert!(verify_id_token(&jwt(t), "web-id", now()).is_ok());
    }

    #[test]
    fn email_verified_accepts_legacy_string_and_defaults_to_false() {
        let mut t = base();
        t["email_verified"] = json!("true");
        assert!(
            verify_id_token(&jwt(t), "web-id", now())
                .unwrap()
                .email_verified
        );
        let mut t = base();
        t.as_object_mut().unwrap().remove("email_verified");
        assert!(
            !verify_id_token(&jwt(t), "web-id", now())
                .unwrap()
                .email_verified
        );
    }

    #[test]
    fn malformed_tokens_are_refused() {
        for bad in [
            "",
            "abc",
            "a.!!!.c",
            &format!("a.{}.c", URL_SAFE_NO_PAD.encode("not json")),
        ] {
            assert!(verify_id_token(bad, "web-id", now()).is_err(), "{bad}");
        }
    }
}
