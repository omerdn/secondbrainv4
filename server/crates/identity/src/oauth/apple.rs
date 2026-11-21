//! Sign in with Apple (T0.3.5).
//!
//! - **Web:** `GET /auth/apple/start` redirects to Apple with `response_mode=form_post`; Apple
//!   POSTs `code`, `state` and — only on the very first authorization — a `user` JSON with the
//!   name to `POST /auth/apple/callback`. Because that POST is cross-site, the state cookie must be
//!   `SameSite=None; Secure`.
//! - **Native (iOS):** `ASAuthorizationAppleIDCredential` gives the app an `authorizationCode`
//!   and — first time only — `fullName`; the app posts them to `POST /auth/apple/native` with its
//!   bundle id as `client_id`.
//!
//! The code is exchanged at Apple's token endpoint with a client secret that is a short-lived
//! ES256 JWT signed by the team's private key (`.p8`). The ID token comes back over TLS from
//! Apple directly (OpenID Connect Core §3.1.3.7); `iss`, `aud` and `exp` are checked.
//!
//! # Apple specifics
//! - **Name only once:** Apple sends the name only on the first authorization. It is used when
//!   the account is created and ignored afterwards; if it is missing, the display name falls back
//!   to the address's local part (the user can change it in the profile, T0.3.7).
//! - **Hidden address (private relay):** the user may share a relay address
//!   (`…@privaterelay.appleid.com`, `is_private_email`). It is a verified, working address and is
//!   stored like any other; it will not match (and so not link to) an existing account registered
//!   with the user's real address. Sending to relay addresses requires registering our sending
//!   domain with Apple (E3.7).
//! - The address may be absent on later sign-ins; a known Apple account does not need it.

use chrono::{DateTime, Duration, Utc};
use kernel::AppError;
use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenType,
};
use oauth2::{
    AuthType, AuthorizationCode, Client, ClientId, ClientSecret, EndpointNotSet, EndpointSet,
    RedirectUrl, StandardRevocableToken, StandardTokenResponse, TokenUrl,
};
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;
use p256::pkcs8::DecodePrivateKey;
use serde::Deserialize;

use super::google::IdTokenFields;
use super::{Provider, ProviderClaims, json_bool, jwt_payload};
use crate::model::CurrentUser;
use crate::service::{IdentityService, IssuedSession};
use crate::token::{self, SessionToken};

/// Apple's authorization endpoint.
pub const AUTH_URL: &str = "https://appleid.apple.com/auth/authorize";
/// Apple's token endpoint.
pub const TOKEN_URL: &str = "https://appleid.apple.com/auth/token";
/// Issuer of Apple ID tokens and audience of client secrets.
pub const ISSUER: &str = "https://appleid.apple.com";
/// How long a browser has to come back from Apple.
pub const STATE_TTL: Duration = Duration::minutes(10);
/// Lifetime of a generated client secret (Apple allows up to six months; we sign per request).
const CLIENT_SECRET_TTL: Duration = Duration::minutes(5);

/// Sign in with Apple settings (Apple Developer account → Certificates, Identifiers & Profiles).
#[derive(Clone)]
pub struct AppleConfig {
    /// Team id (`iss` of the client secret).
    pub team_id: String,
    /// Key id of the Sign in with Apple private key (`kid`).
    pub key_id: String,
    /// The `.p8` private key (PKCS#8 PEM).
    pub private_key_pem: String,
    /// Services ID used by the web flow (`client_id`).
    pub services_id: String,
    /// App bundle ids accepted from native apps.
    pub bundle_ids: Vec<String>,
    /// This server's callback, registered as Return URL of the Services ID.
    pub callback_url: String,
    /// Where the browser goes after a web sign-in.
    pub web_url: String,
    /// Authorization endpoint (overridable for tests).
    pub auth_url: String,
    /// Token endpoint (overridable for tests).
    pub token_url: String,
}

impl std::fmt::Debug for AppleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppleConfig")
            .field("services_id", &self.services_id)
            .finish_non_exhaustive()
    }
}

/// Configured Apple integration.
#[derive(Clone)]
pub struct Apple {
    config: AppleConfig,
    key: SigningKey,
    http: oauth2::reqwest::Client,
}

impl std::fmt::Debug for Apple {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Apple")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// The private key could not be read.
#[derive(Debug, thiserror::Error)]
#[error("invalid Apple private key (expected the .p8 PKCS#8 PEM)")]
pub struct InvalidAppleKey;

impl Apple {
    /// Parses the private key and builds the HTTP client (redirects disabled).
    pub fn new(config: AppleConfig) -> Result<Self, AppError> {
        let key = SigningKey::from_pkcs8_pem(config.private_key_pem.trim())
            .map_err(|_| AppError::internal(InvalidAppleKey))?;
        let http = oauth2::reqwest::Client::builder()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(AppError::internal)?;
        Ok(Self { config, key, http })
    }

    /// ES256 JWT client secret for `client_id` (Apple: "Creating a client secret").
    pub fn client_secret(&self, client_id: &str, now: DateTime<Utc>) -> String {
        use base64::Engine;
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let header = serde_json::json!({ "alg": "ES256", "kid": self.config.key_id });
        let claims = serde_json::json!({
            "iss": self.config.team_id,
            "iat": now.timestamp(),
            "exp": (now + CLIENT_SECRET_TTL).timestamp(),
            "aud": ISSUER,
            "sub": client_id,
        });
        let signing_input = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(claims.to_string().as_bytes())
        );
        let signature: p256::ecdsa::Signature = self.key.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", b64(&signature.to_bytes()))
    }
}

type AppleClient = Client<
    BasicErrorResponse,
    StandardTokenResponse<IdTokenFields, BasicTokenType>,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

fn refused(reason: &'static str) -> AppError {
    tracing::info!(provider = "apple", reason, "provider sign-in refused");
    AppError::Unauthenticated
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    aud: serde_json::Value,
    sub: String,
    exp: i64,
    email: Option<String>,
    email_verified: Option<serde_json::Value>,
}

/// Decodes and checks an ID token received directly from Apple's token endpoint.
pub fn verify_id_token(
    id_token: &str,
    expected_audience: &str,
    now: DateTime<Utc>,
) -> Result<ProviderClaims, AppError> {
    let claims: IdTokenClaims =
        jwt_payload(id_token).ok_or_else(|| refused("malformed id token"))?;
    if claims.iss != ISSUER {
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
    Ok(ProviderClaims {
        subject: claims.sub,
        email: claims.email,
        email_verified: json_bool(claims.email_verified.as_ref()),
        name: None,
        picture: None,
    })
}

/// "Given Family" from optional parts; `None` if both are blank.
pub fn full_name(given: Option<&str>, family: Option<&str>) -> Option<String> {
    let name = [given, family]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!name.is_empty()).then_some(name)
}

/// Name from the web callback's `user` form field (`{"name":{"firstName","lastName"}}`).
pub fn name_from_user_field(user: Option<&str>) -> Option<String> {
    #[derive(Deserialize)]
    struct User {
        name: Option<Name>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Name {
        first_name: Option<String>,
        last_name: Option<String>,
    }
    let user: User = serde_json::from_str(user?).ok()?;
    let name = user.name?;
    full_name(name.first_name.as_deref(), name.last_name.as_deref())
}

/// Validated `POST /auth/apple/native` input.
#[derive(Debug)]
pub struct AppleNativeSignIn {
    pub code: String,
    pub client_id: String,
    pub name: Option<String>,
    pub device: Option<String>,
}

/// Result of starting a browser sign-in.
#[derive(Debug)]
pub struct AppleWebStart {
    /// Apple authorization URL to redirect the browser to.
    pub authorization_url: String,
    /// The `state` value, also set as a cookie.
    pub state: String,
}

impl Apple {
    async fn exchange(
        &self,
        client_id: &str,
        redirect: Option<&str>,
        code: String,
        now: DateTime<Utc>,
    ) -> Result<String, AppError> {
        let mut client: AppleClient = Client::new(ClientId::new(client_id.to_owned()))
            .set_client_secret(ClientSecret::new(self.client_secret(client_id, now)))
            .set_auth_type(AuthType::RequestBody)
            .set_token_uri(
                TokenUrl::new(self.config.token_url.clone()).map_err(AppError::internal)?,
            );
        if let Some(redirect) = redirect {
            client = client.set_redirect_uri(
                RedirectUrl::new(redirect.to_owned()).map_err(AppError::internal)?,
            );
        }
        let response = client
            .exchange_code(AuthorizationCode::new(code))
            .request_async(&self.http)
            .await
            .map_err(|err| {
                tracing::info!(provider = "apple", error = %err, "code exchange failed");
                AppError::Unauthenticated
            })?;
        Ok(response.extra_fields().id_token().to_owned())
    }
}

impl IdentityService {
    fn apple(&self) -> Result<&Apple, AppError> {
        self.inner().apple.as_ref().ok_or(AppError::NotFound)
    }

    /// Whether Sign in with Apple is configured.
    pub fn apple_enabled(&self) -> bool {
        self.inner().apple.is_some()
    }

    /// The web app URL to return to after a browser sign-in, if Apple is configured.
    pub fn apple_web_url(&self) -> Option<String> {
        self.inner()
            .apple
            .as_ref()
            .map(|a| a.config.web_url.trim_end_matches('/').to_owned())
    }

    /// Starts a browser sign-in: stores the state and returns Apple's URL.
    pub async fn apple_web_start(&self) -> Result<AppleWebStart, AppError> {
        let apple = self.apple()?;
        let state = SessionToken::generate().map_err(AppError::internal)?;
        let mut url =
            oauth2::url::Url::parse(&apple.config.auth_url).map_err(AppError::internal)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("response_mode", "form_post")
            .append_pair("client_id", &apple.config.services_id)
            .append_pair("redirect_uri", &apple.config.callback_url)
            .append_pair("scope", "name email")
            .append_pair("state", state.expose());

        let now = self.now();
        sqlx::query(
            "INSERT INTO oauth_states (state_hash, provider, created_at, expires_at)
             VALUES ($1, 'apple', $2, $3)",
        )
        .bind(state.hash().as_slice())
        .bind(now)
        .bind(now + STATE_TTL)
        .execute(self.pool())
        .await
        .map_err(AppError::internal)?;
        Ok(AppleWebStart {
            authorization_url: url.to_string(),
            state: state.expose().to_owned(),
        })
    }

    /// Finishes a browser sign-in from Apple's form POST.
    pub async fn apple_web_callback(
        &self,
        code: String,
        state: &str,
        cookie_state: Option<&str>,
        user_field: Option<&str>,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let apple = self.apple()?;
        if cookie_state != Some(state) {
            return Err(refused("state cookie mismatch"));
        }
        let hash = token::hash_presented(state).ok_or_else(|| refused("malformed state"))?;
        let found = sqlx::query_scalar::<_, i32>(
            "DELETE FROM oauth_states WHERE state_hash = $1 AND provider = 'apple' AND expires_at > $2
             RETURNING 1",
        )
        .bind(hash.as_slice())
        .bind(self.now())
        .fetch_optional(self.pool())
        .await
        .map_err(AppError::internal)?;
        if found.is_none() {
            return Err(refused("unknown or expired state"));
        }

        let config = &apple.config;
        let id_token = apple
            .exchange(
                &config.services_id,
                Some(&config.callback_url),
                code,
                self.now(),
            )
            .await?;
        let mut claims = verify_id_token(&id_token, &config.services_id, self.now())?;
        claims.name = name_from_user_field(user_field);
        self.sign_in_with_provider(Provider::Apple, claims, Some("Web · Apple"))
            .await
    }

    /// Finishes a native sign-in with the app's authorization code.
    pub async fn apple_native(
        &self,
        request: AppleNativeSignIn,
    ) -> Result<(CurrentUser, IssuedSession), AppError> {
        let apple = self.apple()?;
        if !apple.config.bundle_ids.contains(&request.client_id) {
            return Err(refused("unknown bundle id"));
        }
        let id_token = apple
            .exchange(&request.client_id, None, request.code, self.now())
            .await?;
        let mut claims = verify_id_token(&id_token, &request.client_id, self.now())?;
        claims.name = request.name;
        self.sign_in_with_provider(Provider::Apple, claims, request.device.as_deref())
            .await
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;
    use p256::elliptic_curve::Generate;
    use p256::pkcs8::EncodePrivateKey;
    use serde_json::json;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000, 0).unwrap()
    }

    fn config(pem: String) -> AppleConfig {
        AppleConfig {
            team_id: "TEAM123".into(),
            key_id: "KEY123".into(),
            private_key_pem: pem,
            services_id: "com.example.web".into(),
            bundle_ids: vec!["com.example.app".into()],
            callback_url: "https://api.test/auth/apple/callback".into(),
            web_url: "https://app.test".into(),
            auth_url: AUTH_URL.into(),
            token_url: TOKEN_URL.into(),
        }
    }

    #[test]
    fn client_secret_is_a_valid_es256_jwt() {
        use base64::Engine;
        let key = SigningKey::generate();
        let pem = key.to_pkcs8_pem(Default::default()).unwrap().to_string();
        let apple = Apple::new(config(pem)).unwrap();
        let jwt = apple.client_secret("com.example.app", now());

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |p: &str| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(p)
                .unwrap()
        };
        let header: serde_json::Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        assert_eq!(header, json!({ "alg": "ES256", "kid": "KEY123" }));
        let claims: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!(claims["iss"], "TEAM123");
        assert_eq!(claims["sub"], "com.example.app");
        assert_eq!(claims["aud"], ISSUER);
        assert_eq!(
            claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
            300
        );

        let signature = p256::ecdsa::Signature::from_slice(&decode(parts[2])).unwrap();
        let verifying = VerifyingKey::from(&key);
        verifying
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn invalid_key_is_rejected() {
        assert!(Apple::new(config("not a key".into())).is_err());
    }

    fn jwt(claims: serde_json::Value) -> String {
        use base64::Engine;
        let b64 = |s: String| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!(
            "{}.{}.sig",
            b64(json!({}).to_string()),
            b64(claims.to_string())
        )
    }

    #[test]
    fn id_token_checks() {
        let base = json!({
            "iss": ISSUER, "aud": "com.example.app", "sub": "001.abc", "exp": 1_800_000_100,
            "email": "x1@privaterelay.appleid.com", "email_verified": "true", "is_private_email": "true"
        });
        let claims = verify_id_token(&jwt(base.clone()), "com.example.app", now()).unwrap();
        assert_eq!(claims.subject, "001.abc");
        assert!(claims.email_verified);
        assert_eq!(claims.email.as_deref(), Some("x1@privaterelay.appleid.com"));

        assert!(verify_id_token(&jwt(base.clone()), "com.other", now()).is_err());
        let mut t = base.clone();
        t["iss"] = json!("https://accounts.google.com");
        assert!(verify_id_token(&jwt(t), "com.example.app", now()).is_err());
        let mut t = base;
        t["exp"] = json!(1_800_000_000);
        assert!(verify_id_token(&jwt(t), "com.example.app", now()).is_err());
    }

    #[test]
    fn names_from_apple_payloads() {
        assert_eq!(
            full_name(Some(" Ada "), Some("Lovelace")).as_deref(),
            Some("Ada Lovelace")
        );
        assert_eq!(
            full_name(None, Some("Lovelace")).as_deref(),
            Some("Lovelace")
        );
        assert_eq!(full_name(Some(" "), None), None);
        assert_eq!(
            name_from_user_field(Some(
                r#"{"name":{"firstName":"Ada","lastName":"L"},"email":"a@b.c"}"#
            ))
            .as_deref(),
            Some("Ada L")
        );
        assert_eq!(name_from_user_field(Some("not json")), None);
        assert_eq!(name_from_user_field(None), None);
    }
}
