//! Embeds and link previews for pages (PRD Y4, ADR-0013, T2.5.8).
//!
//! `GET /embeds/preview?url=` turns a pasted link into what the editor inserts:
//!
//! - **Embeds** only for the allowed providers (YouTube, Spotify, SoundCloud, Google Drive). The
//!   embed address is built from the link itself, so nothing is fetched and no other origin can
//!   ever end up in an iframe.
//! - **Link previews** for anything else: the server reads the page's title, description and
//!   image (`<title>`, Open Graph). That fetch is a proxy to arbitrary addresses, so it is
//!   guarded against SSRF: http(s) on the default ports only, every resolved address must be
//!   public (no loopback, private, link-local, CGNAT, multicast… in IPv4 or IPv6), the request
//!   goes to the address that was checked (no DNS rebinding), redirects are followed by hand and
//!   checked again (at most [`MAX_REDIRECTS`]), only HTML is read, at most [`MAX_BODY_BYTES`],
//!   within [`FETCH_TIMEOUT`].
//!
//! Nothing is stored and nothing is uploaded (PRD §17).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Redirects followed for one preview.
pub const MAX_REDIRECTS: usize = 3;
/// Bytes of a page read for its preview; the head is near the start.
pub const MAX_BODY_BYTES: usize = 512 * 1024;
/// Time for one whole preview, redirects included.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest link accepted.
pub const MAX_URL_CHARS: usize = 2048;
const MAX_TITLE_CHARS: usize = 300;
const MAX_DESCRIPTION_CHARS: usize = 500;

/// A provider whose content may be embedded (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "EmbedProvider")]
pub enum Provider {
    Youtube,
    Spotify,
    Soundcloud,
    GoogleDrive,
}

/// `GET /embeds/preview` query.
#[derive(Debug, Clone, Deserialize)]
pub struct PreviewQuery {
    pub url: String,
}

/// What the editor inserts for a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, rename = "LinkPreview")]
pub enum Preview {
    /// An allowed provider: shown in an iframe of `embed_url`.
    Embed {
        url: String,
        provider: Provider,
        embed_url: String,
    },
    /// Any other page: a card. Every field may be missing.
    Link {
        url: String,
        title: Option<String>,
        description: Option<String>,
        image: Option<String>,
        site_name: Option<String>,
    },
}

/// Why a link cannot be previewed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PreviewError {
    #[error("not a valid http(s) link")]
    InvalidUrl,
    #[error("this address cannot be fetched")]
    Blocked,
    #[error("the page could not be read")]
    Unreachable,
}

fn host_is(url: &Url, hosts: &[&str]) -> bool {
    url.host_str()
        .is_some_and(|host| hosts.iter().any(|h| host.eq_ignore_ascii_case(h)))
}

fn safe_id(id: &str) -> Option<&str> {
    (!id.is_empty()
        && id.len() <= 100
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(id)
}

fn segments(url: &Url) -> Vec<&str> {
    url.path_segments()
        .map(|s| s.filter(|part| !part.is_empty()).collect())
        .unwrap_or_default()
}

/// The embed for `url` if it belongs to an allowed provider; built from the link alone.
#[must_use]
pub fn embed_for(url: &Url) -> Option<(Provider, String)> {
    let path = segments(url);
    if host_is(url, &["youtube.com", "www.youtube.com", "m.youtube.com"]) {
        let id = match path.as_slice() {
            ["watch"] => url
                .query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned()),
            ["shorts" | "embed" | "live", id, ..] => Some((*id).to_owned()),
            _ => None,
        }?;
        let id = safe_id(&id)?;
        return Some((
            Provider::Youtube,
            format!("https://www.youtube-nocookie.com/embed/{id}"),
        ));
    }
    if host_is(url, &["youtu.be"]) {
        let id = safe_id(path.first()?)?;
        return Some((
            Provider::Youtube,
            format!("https://www.youtube-nocookie.com/embed/{id}"),
        ));
    }
    if host_is(url, &["open.spotify.com"]) {
        // Optional locale prefix: /intl-tr/track/...
        let rest: Vec<&str> = path
            .iter()
            .copied()
            .skip_while(|part| part.starts_with("intl-"))
            .collect();
        if let [
            kind @ ("track" | "album" | "playlist" | "episode" | "show" | "artist"),
            id,
            ..,
        ] = rest.as_slice()
        {
            let id = safe_id(id)?;
            return Some((
                Provider::Spotify,
                format!("https://open.spotify.com/embed/{kind}/{id}"),
            ));
        }
        return None;
    }
    if host_is(
        url,
        &["soundcloud.com", "www.soundcloud.com", "m.soundcloud.com"],
    ) {
        if path.len() < 2 || !path.iter().all(|part| safe_id(part).is_some()) {
            return None;
        }
        let canonical = format!("https://soundcloud.com/{}", path.join("/"));
        let mut player = Url::parse("https://w.soundcloud.com/player/").ok()?;
        player.query_pairs_mut().append_pair("url", &canonical);
        return Some((Provider::Soundcloud, player.to_string()));
    }
    if host_is(url, &["drive.google.com"]) {
        if let ["file", "d", id, ..] = path.as_slice() {
            let id = safe_id(id)?;
            return Some((
                Provider::GoogleDrive,
                format!("https://drive.google.com/file/d/{id}/preview"),
            ));
        }
        return None;
    }
    if host_is(url, &["docs.google.com"])
        && let [
            kind @ ("document" | "spreadsheets" | "presentation" | "forms"),
            "d",
            id,
            ..,
        ] = path.as_slice()
    {
        let id = safe_id(id)?;
        return Some((
            Provider::GoogleDrive,
            format!("https://docs.google.com/{kind}/d/{id}/preview"),
        ));
    }
    None
}

/// Parses a pasted link: http(s), a host, no credentials, not too long.
///
/// # Errors
/// [`PreviewError::InvalidUrl`].
pub fn parse_link(raw: &str) -> Result<Url, PreviewError> {
    let raw = raw.trim();
    if raw.chars().count() > MAX_URL_CHARS {
        return Err(PreviewError::InvalidUrl);
    }
    let url = Url::parse(raw).map_err(|_| PreviewError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(PreviewError::InvalidUrl);
    }
    Ok(url)
}

/// Whether the server may connect to `ip`: only public unicast addresses.
#[must_use]
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // fc00::/7 unique local, fe80::/10 link-local, fec0::/10 site-local
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                // 64:ff9b::/96 NAT64 could reach IPv4 inside; 2001:db8::/32 documentation
                || (s[0] == 0x64 && s[1] == 0xff9b)
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                // 2002::/16 6to4 embeds an IPv4 address
                || s[0] == 0x2002)
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        // 100.64.0.0/10 shared (CGNAT)
        || (a == 100 && (b & 0xc0) == 64)
        // 192.0.0.0/24 protocol assignments
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15 benchmarking
        || (a == 198 && (b & 0xfe) == 18)
        // 240.0.0.0/4 reserved
        || a >= 240)
}

/// Which addresses the fetcher may reach. Production allows public addresses on the default
/// ports only; tests also allow loopback to serve pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchPolicy {
    pub allow_loopback: bool,
}

impl FetchPolicy {
    pub const PUBLIC: Self = Self {
        allow_loopback: false,
    };

    fn allows(self, addr: SocketAddr) -> bool {
        if self.allow_loopback && addr.ip().is_loopback() {
            return true;
        }
        is_public(addr.ip()) && matches!(addr.port(), 80 | 443)
    }
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self::PUBLIC
    }
}

/// The first allowed address of `url`'s host, or why there is none.
async fn resolve(url: &Url, policy: FetchPolicy) -> Result<SocketAddr, PreviewError> {
    let host = url.host_str().ok_or(PreviewError::InvalidUrl)?;
    let port = url
        .port_or_known_default()
        .ok_or(PreviewError::InvalidUrl)?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| PreviewError::Unreachable)?
            .collect()
    };
    if addrs.is_empty() {
        return Err(PreviewError::Unreachable);
    }
    // Every address must pass: a name that also points inside is refused.
    if !addrs.iter().all(|addr| policy.allows(*addr)) {
        return Err(PreviewError::Blocked);
    }
    addrs.first().copied().ok_or(PreviewError::Unreachable)
}

/// Reads the page at `url` and returns its HTML start, following checked redirects.
async fn fetch_html(url: Url, policy: FetchPolicy) -> Result<(Url, String), PreviewError> {
    let mut current = url;
    for _ in 0..=MAX_REDIRECTS {
        let addr = resolve(&current, policy).await?;
        let host = current
            .host_str()
            .ok_or(PreviewError::InvalidUrl)?
            .to_owned();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, addr)
            .timeout(FETCH_TIMEOUT)
            .user_agent("RoutuneLinkPreview/1.0")
            .build()
            .map_err(|_| PreviewError::Unreachable)?;
        let mut response = client
            .get(current.clone())
            .header(reqwest::header::ACCEPT, "text/html")
            .send()
            .await
            .map_err(|_| PreviewError::Unreachable)?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(PreviewError::Unreachable)?;
            let next = current
                .join(location)
                .map_err(|_| PreviewError::Unreachable)?;
            current = parse_link(next.as_str())?;
            continue;
        }
        if !response.status().is_success() {
            return Err(PreviewError::Unreachable);
        }
        let is_html = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"));
        if !is_html {
            return Err(PreviewError::Unreachable);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| PreviewError::Unreachable)?
        {
            let room = MAX_BODY_BYTES - body.len();
            body.extend_from_slice(&chunk[..chunk.len().min(room)]);
            if body.len() >= MAX_BODY_BYTES {
                break;
            }
        }
        return Ok((current, String::from_utf8_lossy(&body).into_owned()));
    }
    Err(PreviewError::Unreachable)
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

fn clean(text: &str, max: usize) -> Option<String> {
    let text = decode_entities(text);
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(max).collect())
}

/// The value of `name="..."` in one tag's text.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = lower[at + name.len()..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let offset = tag.len() - rest.len() + 1;
        let value = tag[offset..].trim_start();
        let quote = value.chars().next()?;
        return if quote == '"' || quote == '\'' {
            value[1..].split(quote).next().map(str::to_owned)
        } else {
            value
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .map(str::to_owned)
        };
    }
    None
}

/// Title, description, image and site name from a page's HTML (Open Graph first).
#[must_use]
pub fn read_meta(
    html: &str,
    base: &Url,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let lower = html.to_ascii_lowercase();
    let mut og = std::collections::HashMap::new();
    let mut description = None;
    let mut from = 0;
    while let Some(found) = lower[from..].find("<meta") {
        let start = from + found;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &html[start..end];
        from = end;
        let key = attribute(tag, "property").or_else(|| attribute(tag, "name"));
        let Some(content) = attribute(tag, "content") else {
            continue;
        };
        match key.map(|k| k.to_ascii_lowercase()).as_deref() {
            Some(k @ ("og:title" | "og:description" | "og:image" | "og:site_name")) => {
                og.entry(k.to_owned()).or_insert(content);
            }
            Some("description") => {
                description.get_or_insert(content);
            }
            _ => {}
        }
    }
    let title_tag = lower.find("<title").and_then(|start| {
        let open = start + lower[start..].find('>')? + 1;
        let close = open + lower[open..].find("</title")?;
        Some(html[open..close].to_owned())
    });
    let title = og
        .get("og:title")
        .cloned()
        .or(title_tag)
        .and_then(|t| clean(&t, MAX_TITLE_CHARS));
    let description = og
        .get("og:description")
        .cloned()
        .or(description)
        .and_then(|d| clean(&d, MAX_DESCRIPTION_CHARS));
    // Only https images; the browser loads them directly.
    let image = og
        .get("og:image")
        .and_then(|src| base.join(&decode_entities(src)).ok())
        .filter(|src| src.scheme() == "https")
        .map(|src| src.to_string())
        .filter(|src| src.chars().count() <= MAX_URL_CHARS);
    let site_name = og
        .get("og:site_name")
        .and_then(|s| clean(s, MAX_TITLE_CHARS));
    (title, description, image, site_name)
}

/// The preview of a pasted link (see the module docs).
///
/// # Errors
/// [`PreviewError`].
pub async fn preview(raw: &str, policy: FetchPolicy) -> Result<Preview, PreviewError> {
    let url = parse_link(raw)?;
    if let Some((provider, embed_url)) = embed_for(&url) {
        return Ok(Preview::Embed {
            url: url.to_string(),
            provider,
            embed_url,
        });
    }
    let (final_url, html) = tokio::time::timeout(FETCH_TIMEOUT, fetch_html(url.clone(), policy))
        .await
        .map_err(|_| PreviewError::Unreachable)??;
    let (title, description, image, site_name) = read_meta(&html, &final_url);
    Ok(Preview::Link {
        url: url.to_string(),
        title,
        description,
        image,
        site_name,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn embed(raw: &str) -> Option<(Provider, String)> {
        embed_for(&parse_link(raw).unwrap())
    }

    #[test]
    fn y4_allowed_providers_embed_from_the_link_alone() {
        let yt = Some((
            Provider::Youtube,
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ".to_owned(),
        ));
        assert_eq!(
            embed("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10"),
            yt
        );
        assert_eq!(embed("https://youtu.be/dQw4w9WgXcQ"), yt);
        assert_eq!(embed("https://youtube.com/shorts/dQw4w9WgXcQ"), yt);
        assert_eq!(
            embed("https://open.spotify.com/intl-tr/track/4uLU6hMCjMI75M1A2tKUQC?si=x"),
            Some((
                Provider::Spotify,
                "https://open.spotify.com/embed/track/4uLU6hMCjMI75M1A2tKUQC".to_owned()
            ))
        );
        assert_eq!(
            embed("https://soundcloud.com/artist/a-song"),
            Some((
                Provider::Soundcloud,
                "https://w.soundcloud.com/player/?url=https%3A%2F%2Fsoundcloud.com%2Fartist%2Fa-song"
                    .to_owned()
            ))
        );
        assert_eq!(
            embed("https://drive.google.com/file/d/1AbC_d-9/view?usp=sharing"),
            Some((
                Provider::GoogleDrive,
                "https://drive.google.com/file/d/1AbC_d-9/preview".to_owned()
            ))
        );
        assert_eq!(
            embed("https://docs.google.com/document/d/1AbC/edit"),
            Some((
                Provider::GoogleDrive,
                "https://docs.google.com/document/d/1AbC/preview".to_owned()
            ))
        );
    }

    #[test]
    fn y4_anything_else_is_not_embedded() {
        for raw in [
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.evil.example/watch?v=x",
            "https://www.youtube.com/watch?v=<script>",
            "https://www.youtube.com/channel/abc",
            "https://open.spotify.com/user/abc",
            "https://soundcloud.com/artist",
            "https://drive.google.com/drive/folders/abc",
            "https://evilyoutube.com/watch?v=abc",
        ] {
            assert_eq!(embed(raw), None, "{raw}");
        }
        for bad in [
            "javascript:alert(1)",
            "ftp://example.com/x",
            "https://user:pass@example.com/",
            "not a url",
            "file:///etc/passwd",
        ] {
            assert_eq!(parse_link(bad), Err(PreviewError::InvalidUrl), "{bad}");
        }
        assert_eq!(
            parse_link(&format!(
                "https://example.com/{}",
                "a".repeat(MAX_URL_CHARS)
            )),
            Err(PreviewError::InvalidUrl)
        );
    }

    #[test]
    fn only_public_addresses_are_reachable() {
        for inside in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "198.18.0.1",
            "192.0.0.8",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "2002:a00:1::1",
            "ff02::1",
        ] {
            assert!(!is_public(inside.parse().unwrap()), "{inside}");
        }
        for outside in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(outside.parse().unwrap()), "{outside}");
        }
        let public = FetchPolicy::PUBLIC;
        assert!(public.allows("93.184.216.34:443".parse().unwrap()));
        assert!(!public.allows("93.184.216.34:8080".parse().unwrap()));
        assert!(!public.allows("127.0.0.1:443".parse().unwrap()));
    }

    #[test]
    fn meta_prefers_open_graph_and_cleans_text() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let html = r#"<html><head><title> Sayfa &amp; Başlık </title>
            <meta name="description" content="Düz açıklama">
            <meta property="og:description" content="OG açıklama">
            <meta content="/img/k.png" property="og:image">
            <meta property='og:site_name' content='Örnek'>
            </head><body>...</body></html>"#;
        let (title, description, image, site) = read_meta(html, &base);
        assert_eq!(title.as_deref(), Some("Sayfa & Başlık"));
        assert_eq!(description.as_deref(), Some("OG açıklama"));
        assert_eq!(image.as_deref(), Some("https://example.com/img/k.png"));
        assert_eq!(site.as_deref(), Some("Örnek"));
        // An insecure image is dropped; missing parts stay missing.
        let (title, description, image, _) = read_meta(
            r#"<meta property="og:image" content="http://x.example/i.png">"#,
            &base,
        );
        assert_eq!((title, description, image), (None, None, None));
    }
}
