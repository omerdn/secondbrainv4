//! Self-hosted Expo Updates, protocol v1 (T4.2.6, ADR-0023).
//!
//! The installed mobile apps (`expo-updates`) ask `GET /updates/manifest` on launch with their
//! platform and runtime version (a fingerprint of the native layer). When a newer JS update
//! exists for exactly that runtime, the manifest lists its bundle and assets, served from
//! `GET /updates/assets/...`; the app downloads them and applies the update on the next launch.
//! A new native layer has a new runtime version, so it never receives JS built for another.
//!
//! Updates are the output of `expo export --platform <p>` copied by the deploy to
//! `<UPDATES_DIR>/<platform>/<runtime>/<update-id>/` (with its `metadata.json`). The newest
//! update is the directory whose name sorts last (the deploy names them by UTC timestamp).
//! No database: files only.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

/// Boundary of the multipart manifest response.
const BOUNDARY: &str = "routune-expo-update";

/// Where updates live and how clients reach their assets.
#[derive(Debug, Clone)]
pub struct UpdatesConfig {
    /// `UPDATES_DIR`.
    pub dir: PathBuf,
    /// Public origin of this server (`PUBLIC_API_URL`), for asset URLs.
    pub public_url: String,
}

/// `/updates/manifest` and `/updates/assets/...`.
pub fn router(config: UpdatesConfig) -> Router {
    Router::new()
        .route("/updates/manifest", get(manifest))
        .route(
            "/updates/assets/{platform}/{runtime}/{update}/{*path}",
            get(asset),
        )
        .with_state(Arc::new(config))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Ios,
    Android,
}

impl Platform {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "ios" => Some(Self::Ios),
            "android" => Some(Self::Android),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// Runtime versions and update ids are single path segments of safe characters.
fn is_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(rename = "fileMetadata")]
    file_metadata: std::collections::HashMap<String, PlatformFiles>,
}

#[derive(Deserialize)]
struct PlatformFiles {
    bundle: String,
    #[serde(default)]
    assets: Vec<AssetFile>,
}

#[derive(Deserialize)]
struct AssetFile {
    path: String,
    ext: String,
}

async fn manifest(State(config): State<Arc<UpdatesConfig>>, headers: HeaderMap) -> Response {
    let platform = header_str(&headers, "expo-platform").and_then(Platform::parse);
    let runtime = header_str(&headers, "expo-runtime-version").filter(|r| is_segment(r));
    let (Some(platform), Some(runtime)) = (platform, runtime) else {
        return (
            StatusCode::BAD_REQUEST,
            "expo-platform and expo-runtime-version required",
        )
            .into_response();
    };

    match build_manifest(&config, platform, runtime).await {
        Ok(Some(body)) => manifest_response(&body),
        // No update for this runtime: the app keeps what it has.
        Ok(None) => (StatusCode::NO_CONTENT, protocol_headers()).into_response(),
        Err(err) => {
            tracing::error!(platform = platform.as_str(), error = %err, "update manifest failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn protocol_headers() -> [(&'static str, HeaderValue); 3] {
    [
        ("expo-protocol-version", HeaderValue::from_static("1")),
        ("expo-sfv-version", HeaderValue::from_static("0")),
        (
            header::CACHE_CONTROL.as_str(),
            HeaderValue::from_static("private, max-age=0"),
        ),
    ]
}

fn manifest_response(manifest: &serde_json::Value) -> Response {
    let body = format!(
        "--{BOUNDARY}\r\ncontent-disposition: form-data; name=\"manifest\"\r\n\
         content-type: application/json; charset=utf-8\r\n\r\n{manifest}\r\n--{BOUNDARY}--\r\n"
    );
    let mut response = (protocol_headers(), body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("multipart/mixed; boundary=routune-expo-update"),
    );
    response
}

/// Newest update directory of a platform and runtime, if any.
async fn latest_update(dir: &Path) -> std::io::Result<Option<String>> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut newest: Option<String> = None;
    while let Some(entry) = entries.next_entry().await? {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if is_segment(&name)
            && entry.file_type().await?.is_dir()
            && newest.as_deref().is_none_or(|n| name.as_str() > n)
        {
            newest = Some(name);
        }
    }
    Ok(newest)
}

async fn build_manifest(
    config: &UpdatesConfig,
    platform: Platform,
    runtime: &str,
) -> Result<Option<serde_json::Value>, String> {
    let runtime_dir = config.dir.join(platform.as_str()).join(runtime);
    let Some(update) = latest_update(&runtime_dir)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let update_dir = runtime_dir.join(&update);
    let metadata_bytes = tokio::fs::read(update_dir.join("metadata.json"))
        .await
        .map_err(|e| format!("metadata.json: {e}"))?;
    let metadata: Metadata =
        serde_json::from_slice(&metadata_bytes).map_err(|e| format!("metadata.json: {e}"))?;
    let Some(files) = metadata.file_metadata.get(platform.as_str()) else {
        return Ok(None);
    };

    let base = format!(
        "{}/updates/assets/{}/{runtime}/{update}",
        config.public_url.trim_end_matches('/'),
        platform.as_str()
    );
    let describe = |path: &str, bytes: &[u8], content_type: &str| {
        let digest = Sha256::digest(bytes);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        json!({
            "hash": URL_SAFE_NO_PAD.encode(digest),
            "key": &hex[..32],
            "contentType": content_type,
            "url": format!("{base}/{path}"),
        })
    };

    let bundle = read_inside(&update_dir, &files.bundle).await?;
    let launch_asset = describe(&files.bundle, &bundle, "application/javascript");
    let mut assets = Vec::with_capacity(files.assets.len());
    for file in &files.assets {
        let bytes = read_inside(&update_dir, &file.path).await?;
        let mut asset = describe(&file.path, &bytes, content_type(&file.ext));
        asset["fileExtension"] = json!(format!(".{}", file.ext));
        assets.push(asset);
    }

    let id_digest = Sha256::digest(&metadata_bytes);
    let mut id_bytes = [0_u8; 16];
    id_bytes.copy_from_slice(&id_digest[..16]);
    let created_at = tokio::fs::metadata(&update_dir)
        .await
        .and_then(|m| m.modified())
        .map(chrono::DateTime::<chrono::Utc>::from)
        .unwrap_or_else(|_| chrono::Utc::now());

    Ok(Some(json!({
        "id": uuid::Builder::from_random_bytes(id_bytes).into_uuid().to_string(),
        "createdAt": created_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "runtimeVersion": runtime,
        "launchAsset": launch_asset,
        "assets": assets,
        "metadata": {},
        "extra": {},
    })))
}

/// Reads `relative` below `root`, refusing absolute paths and `..` (paths come from files the
/// deploy wrote, and from request URLs for assets).
async fn read_inside(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    let path = Path::new(relative);
    if relative.is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("invalid asset path `{relative}`"));
    }
    tokio::fs::read(root.join(path))
        .await
        .map_err(|e| format!("{relative}: {e}"))
}

fn content_type(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "js" | "hbc" | "bundle" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn asset(
    State(config): State<Arc<UpdatesConfig>>,
    UrlPath((platform, runtime, update, path)): UrlPath<(String, String, String, String)>,
) -> Response {
    let Some(platform) = Platform::parse(&platform) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !is_segment(&runtime) || !is_segment(&update) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = config
        .dir
        .join(platform.as_str())
        .join(&runtime)
        .join(&update);
    let Ok(bytes) = read_inside(&root, &path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    (
        [
            (header::CONTENT_TYPE, content_type(ext)),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An updates dir with one export for android runtime `rt1` (two updates; newest wins).
    fn fixture() -> TempDir {
        let root = std::env::temp_dir().join(format!("updates-test-{}", uuid::Uuid::now_v7()));
        for (update, bundle) in [("20261101T000000Z", "old"), ("20261122T120000Z", "new")] {
            let dir = root.join("android/rt1").join(update);
            std::fs::create_dir_all(dir.join("_expo/static/js/android")).unwrap();
            std::fs::create_dir_all(dir.join("assets")).unwrap();
            std::fs::write(dir.join("_expo/static/js/android/entry.hbc"), bundle).unwrap();
            std::fs::write(dir.join("assets/abc"), b"png-bytes").unwrap();
            std::fs::write(
                dir.join("metadata.json"),
                format!(
                    r#"{{"version":0,"bundler":"metro","fileMetadata":{{"android":{{"bundle":"_expo/static/js/android/entry.hbc","assets":[{{"path":"assets/abc","ext":"png"}}]}}}},"x":"{bundle}"}}"#
                ),
            )
            .unwrap();
        }
        TempDir(root)
    }

    fn app(dir: &Path) -> Router {
        router(UpdatesConfig {
            dir: dir.to_owned(),
            public_url: "https://api.test/".into(),
        })
    }

    async fn get(
        app: Router,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut request = Request::get(uri);
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, headers, body)
    }

    fn manifest_json(body: &[u8]) -> serde_json::Value {
        let text = String::from_utf8(body.to_vec()).unwrap();
        let start = text.find("\r\n\r\n").unwrap() + 4;
        let end = text.rfind("\r\n--routune-expo-update--").unwrap();
        serde_json::from_str(&text[start..end]).unwrap()
    }

    #[tokio::test]
    async fn serves_the_newest_update_of_the_runtime() {
        let dir = fixture();
        let (status, headers, body) = get(
            app(&dir.0),
            "/updates/manifest",
            &[
                ("expo-platform", "android"),
                ("expo-runtime-version", "rt1"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["expo-protocol-version"], "1");
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("multipart/mixed; boundary=")
        );
        let manifest = manifest_json(&body);
        assert_eq!(manifest["runtimeVersion"], "rt1");
        let launch = &manifest["launchAsset"];
        assert_eq!(
            launch["url"],
            "https://api.test/updates/assets/android/rt1/20261122T120000Z/_expo/static/js/android/entry.hbc"
        );
        assert_eq!(
            launch["hash"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"new"))
        );
        assert_eq!(manifest["assets"][0]["contentType"], "image/png");
        assert_eq!(manifest["assets"][0]["fileExtension"], ".png");
        assert_eq!(
            uuid::Uuid::parse_str(manifest["id"].as_str().unwrap())
                .unwrap()
                .get_version_num(),
            4
        );
    }

    #[tokio::test]
    async fn other_runtimes_and_platforms_get_no_update() {
        let dir = fixture();
        for (platform, runtime) in [("android", "rt2"), ("ios", "rt1")] {
            let (status, headers, _) = get(
                app(&dir.0),
                "/updates/manifest",
                &[
                    ("expo-platform", platform),
                    ("expo-runtime-version", runtime),
                ],
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT, "{platform}/{runtime}");
            assert_eq!(headers["expo-protocol-version"], "1");
        }
    }

    #[tokio::test]
    async fn rejects_missing_or_unsafe_headers() {
        let dir = fixture();
        for headers in [
            vec![("expo-runtime-version", "rt1")],
            vec![("expo-platform", "web"), ("expo-runtime-version", "rt1")],
            vec![
                ("expo-platform", "android"),
                ("expo-runtime-version", "../rt1"),
            ],
        ] {
            let (status, _, _) = get(app(&dir.0), "/updates/manifest", &headers).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{headers:?}");
        }
    }

    #[tokio::test]
    async fn serves_assets_and_refuses_traversal() {
        let dir = fixture();
        let (status, _, body) = get(
            app(&dir.0),
            "/updates/assets/android/rt1/20261122T120000Z/assets/abc",
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"png-bytes");
        for uri in [
            "/updates/assets/android/rt1/20261122T120000Z/..%2F..%2Fmetadata.json",
            "/updates/assets/android/..%2Frt1/20261122T120000Z/assets/abc",
            "/updates/assets/web/rt1/20261122T120000Z/assets/abc",
        ] {
            let (status, _, _) = get(app(&dir.0), uri, &[]).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
    }
}
