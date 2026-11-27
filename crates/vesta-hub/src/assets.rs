//! The wall-panel UI, served on the same origin as the API.
//!
//! The local endpoint serves both, so the frontend keeps using same-origin
//! relative URLs on every target. Where the files come from is up to the
//! embedder ([`UiAssets`]): the Tauri shell uses its embedded frontend,
//! `vesta-hubd --ui` a built `web/dist` read into memory ([`UiFiles`]).
//!
//! - `GET`/`HEAD` only. `/api/...` is never answered from here.
//! - Unknown paths without a file extension get `index.html` (client-side
//!   routes); unknown files are `404`.
//! - Paths are percent-decoded and checked before the source sees them:
//!   no `.`/`..`/empty segments, backslashes or control characters.
//! - Every response carries a strict `Content-Security-Policy`,
//!   `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::header::{ALLOW, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use percent_encoding::percent_decode_str;

/// One file of the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiAsset {
    pub bytes: Bytes,
    /// `Content-Type`, e.g. `text/javascript; charset=utf-8`.
    pub mime: String,
}

/// Where the UI files come from.
pub trait UiAssets: Send + Sync + 'static {
    /// The file at `path`, e.g. `index.html` or `assets/index-1a2b.js`.
    /// `path` is relative and already checked: `/`-separated, no `.`,
    /// `..` or empty segments, no backslashes or control characters.
    fn get(&self, path: &str) -> Option<UiAsset>;

    /// `connect-src` sources the UI needs besides its own origin, e.g. the
    /// app shell's native bridge. Keep it to what the shell really uses.
    fn extra_connect_src(&self) -> &[&'static str] {
        &[]
    }
}

impl fmt::Debug for dyn UiAssets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UiAssets")
    }
}

/// UI files held in memory, e.g. a built `web/dist` read at start-up.
#[derive(Debug, Clone, Default)]
pub struct UiFiles {
    files: BTreeMap<String, UiAsset>,
}

impl UiFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) `path`, typed by its extension.
    pub fn insert(&mut self, path: impl Into<String>, bytes: impl Into<Bytes>) -> &mut Self {
        let path = path.into();
        let mime = mime_for(&path).to_owned();
        self.files.insert(
            path,
            UiAsset {
                bytes: bytes.into(),
                mime,
            },
        );
        self
    }

    /// Reads every regular file under `dir` (a built `web/dist`), skipping
    /// hidden files. Fails if there is no `index.html`.
    pub fn load_dir(dir: &Path) -> std::io::Result<Self> {
        fn walk(dir: &Path, prefix: &str, files: &mut UiFiles) -> std::io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                let path = format!("{prefix}{name}");
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    walk(&entry.path(), &format!("{path}/"), files)?;
                } else if kind.is_file() {
                    files.insert(path, std::fs::read(entry.path())?);
                }
            }
            Ok(())
        }
        let mut files = Self::new();
        walk(dir, "", &mut files)?;
        if !files.files.contains_key(INDEX) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} has no {INDEX}", dir.display()),
            ));
        }
        Ok(files)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl UiAssets for UiFiles {
    fn get(&self, path: &str) -> Option<UiAsset> {
        self.files.get(path).cloned()
    }
}

const INDEX: &str = "index.html";

/// `Content-Type` for the file types a Vite build produces.
pub fn mime_for(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    match extension.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// The policy for every UI response. Scripts only from this origin; inline
/// styles are allowed because UI libraries insert `<style>` elements at
/// runtime (toasts, scroll locking).
pub(crate) fn content_security_policy(extra_connect_src: &[&str]) -> String {
    let mut connect = String::from("'self'");
    for source in extra_connect_src {
        // Static strings from the embedder; keep the header well-formed.
        if !source.is_empty() && !source.contains([';', ',', '\r', '\n']) {
            connect.push(' ');
            connect.push_str(source);
        }
    }
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self'; font-src 'self'; connect-src {connect}; object-src 'none'; \
         base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    )
}

/// The request path as a checked relative path (`index.html` for `/`).
fn checked_path(raw: &str) -> Option<String> {
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    let relative = decoded.strip_prefix('/')?;
    if relative.is_empty() {
        return Some(INDEX.to_owned());
    }
    let relative = relative.strip_suffix('/').unwrap_or(relative);
    let valid = relative.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.contains('\\')
            && !segment.chars().any(char::is_control)
    });
    valid.then(|| relative.to_owned())
}

/// `assets/app.js` names a file; `rooms/kitchen` is a client-side route.
fn names_a_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
}

fn is_api(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// Serves the UI; see the module docs.
#[derive(Clone)]
pub(crate) struct UiService {
    assets: Arc<dyn UiAssets>,
    csp: HeaderValue,
}

impl UiService {
    pub(crate) fn new(assets: Arc<dyn UiAssets>) -> anyhow::Result<Self> {
        let csp = HeaderValue::from_str(&content_security_policy(assets.extra_connect_src()))?;
        Ok(Self { assets, csp })
    }

    pub(crate) fn respond(&self, method: &Method, uri: &Uri) -> Response {
        if method != Method::GET && method != Method::HEAD {
            return (
                StatusCode::METHOD_NOT_ALLOWED,
                [(ALLOW, HeaderValue::from_static("GET, HEAD"))],
            )
                .into_response();
        }
        if is_api(uri.path()) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let Some(path) = checked_path(uri.path()) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let found = match self.assets.get(&path) {
            Some(asset) => Some((path, asset)),
            // A client-side route, not a missing file.
            None if !names_a_file(&path) => self
                .assets
                .get(INDEX)
                .map(|asset| (INDEX.to_owned(), asset)),
            None => None,
        };
        let Some((path, asset)) = found else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let Ok(content_type) = HeaderValue::from_str(&asset.mime) else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        // Vite fingerprints everything under `assets/`.
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        (
            [
                (CONTENT_TYPE, content_type),
                (CACHE_CONTROL, HeaderValue::from_static(cache)),
                (CONTENT_SECURITY_POLICY, self.csp.clone()),
            ],
            Body::from(asset.bytes),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_paths() {
        assert_eq!(checked_path("/").as_deref(), Some("index.html"));
        assert_eq!(
            checked_path("/assets/index-1a2b.js").as_deref(),
            Some("assets/index-1a2b.js")
        );
        assert_eq!(
            checked_path("/rooms/kitchen/").as_deref(),
            Some("rooms/kitchen")
        );
        assert_eq!(checked_path("/Sal%C3%B3n").as_deref(), Some("Salón"));
        for bad in [
            "",
            "assets/x.js",
            "/../secret",
            "/assets/../../secret",
            "/assets/%2e%2e/secret",
            "/assets/%2E%2E%2Fsecret",
            "/assets/..%2fsecret",
            "/./index.html",
            "//index.html",
            "/assets//x.js",
            "/assets\\..\\secret",
            "/assets/%5c..%5csecret",
            "/a%00b",
            "/%ff",
        ] {
            assert_eq!(checked_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn types_files_by_extension() {
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("assets/a.JS"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("assets/geist.woff2"), "font/woff2");
        assert_eq!(mime_for("favicon.svg"), "image/svg+xml");
        assert_eq!(mime_for("LICENSE"), "application/octet-stream");
    }

    #[test]
    fn the_policy_is_strict() {
        let csp = content_security_policy(&["ipc:", "bad; script-src *", "http://ipc.localhost"]);
        assert!(csp.starts_with("default-src 'self'; script-src 'self';"));
        assert!(csp.contains("connect-src 'self' ipc: http://ipc.localhost;"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("script-src *"));
        assert!(!csp.contains("unsafe-eval"));
    }
}
