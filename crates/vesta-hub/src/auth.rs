//! Who is calling: one-time login codes, the session cookie and the
//! [`Identity`] every API handler can rely on.
//!
//! Loopback is reachable by every program on the device, so the Hub does
//! not trust a request just because it is local (see
//! `docs/architecture/local-session.md`):
//!
//! 1. The embedding shell (the Tauri app, or `vesta-hubd` printing a URL)
//!    asks for a [`LoginCode`]: 128 bits from the OS CSPRNG, single use,
//!    valid for [`LOGIN_CODE_TTL`].
//! 2. The panel opens `GET /api/v1/session?code=…`. The Hub redeems the code
//!    (constant-time compare), creates a 256-bit session, sets the
//!    `vesta_session` cookie (`HttpOnly; SameSite=Strict; Path=/`) and
//!    answers `303 → /`. No secret ever reaches JavaScript.
//! 3. Every other `/api/v1/*` route, the WebSocket included, needs that
//!    cookie, except `/api/v1/health`. Cookies ignore ports (RFC 6265
//!    §8.5), so a session is only honoured when `Host` and `Origin` are
//!    *exactly* one of the allowed origins: the Hub's own
//!    (`http://127.0.0.1:<port>`, `http://localhost:<port>`) plus any
//!    configured explicitly ([`crate::HubOptions::allowed_origins`], e.g.
//!    the Vite dev server). Another server on another local port is not.
//!
//! This module is the single place that decides who a request comes from:
//! it inserts an [`Identity`] request extension, and handlers never look at
//! cookies themselves. `vesta-link` will add identities for paired phones.

use std::collections::VecDeque;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::bail;
use axum::Router;
use axum::extract::{MatchedPath, RawQuery, Request, State};
use axum::http::header::{
    CACHE_CONTROL, COOKIE, HOST, LOCATION, ORIGIN, REFERRER_POLICY, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use subtle::ConstantTimeEq;

use crate::api::{host_of, is_local_host};

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "vesta_session";
/// Where a login code is redeemed: `GET /api/v1/session?code=…`.
pub const LOGIN_PATH: &str = "/api/v1/session";
/// How long a login code stays valid.
pub const LOGIN_CODE_TTL: Duration = Duration::from_secs(60);

/// Routes that never need a session.
const PUBLIC_ROUTES: [&str; 2] = ["/api/v1/health", LOGIN_PATH];
const LOGIN_CODE_BYTES: usize = 16;
const SESSION_ID_BYTES: usize = 32;
/// Unredeemed codes kept at once; issuing more drops the oldest.
const MAX_PENDING_CODES: usize = 8;
/// Sessions kept at once; a new one beyond this drops the oldest.
const MAX_SESSIONS: usize = 32;

/// How the API decides who is calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiAuth {
    /// No authentication: every local request acts as an admin. Only for
    /// the development loop (`vesta-hubd --dev`), never the default.
    Disabled,
    /// One-time login code, then a session cookie. The default.
    #[default]
    Sessions,
}

/// What a caller may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Member,
    Guest,
}

/// Where a caller comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    /// Authentication is disabled (development only).
    Dev,
    /// The panel on this device, signed in with a login code from its shell.
    LocalPanel,
}

/// Who a request comes from. Inserted as a request extension on every
/// `/api/v1/*` route that needs a session; extract it with
/// `Extension<Identity>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub kind: IdentityKind,
    pub role: Role,
}

impl Identity {
    pub const DEV: Self = Self {
        kind: IdentityKind::Dev,
        role: Role::Admin,
    };
    pub const LOCAL_PANEL: Self = Self {
        kind: IdentityKind::LocalPanel,
        role: Role::Admin,
    };
}

/// A one-time login code. `Debug` never prints it.
pub struct LoginCode {
    code: String,
}

impl LoginCode {
    /// The code itself (base64url, no padding).
    pub fn as_str(&self) -> &str {
        &self.code
    }

    /// Path and query that redeem it, e.g. `/api/v1/session?code=…`.
    pub fn login_path(&self) -> String {
        format!("{LOGIN_PATH}?code={}", self.code)
    }
}

impl fmt::Debug for LoginCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoginCode(<redacted>)")
    }
}

/// Issues login codes for a running Hub. Cheap to clone.
#[derive(Clone)]
pub struct LoginCodes(Arc<SessionStore>);

impl LoginCodes {
    /// A fresh single-use code, valid for [`LOGIN_CODE_TTL`].
    pub fn issue(&self) -> anyhow::Result<LoginCode> {
        self.0.issue_code_at(Instant::now())
    }
}

impl fmt::Debug for LoginCodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoginCodes")
    }
}

/// The Hub's authentication settings and session store.
pub(crate) struct Auth {
    /// `None`: authentication disabled.
    store: Option<Arc<SessionStore>>,
    /// Extra exact origins (normalized), besides the Hub's own.
    allowed_origins: Vec<String>,
}

impl Auth {
    pub(crate) fn new(mode: ApiAuth, allowed_origins: &[String]) -> anyhow::Result<Self> {
        let allowed_origins = allowed_origins
            .iter()
            .map(|origin| normalize_origin(origin))
            .collect::<anyhow::Result<_>>()?;
        Ok(Self {
            store: match mode {
                ApiAuth::Disabled => None,
                ApiAuth::Sessions => Some(Arc::new(SessionStore::default())),
            },
            allowed_origins,
        })
    }

    pub(crate) fn login_codes(&self) -> Option<LoginCodes> {
        self.store.clone().map(LoginCodes)
    }

    /// The rules for a router served on `local` (`None`: not bound to a
    /// socket, so only the configured origins are known and `Host` is
    /// only checked by the local-only guard).
    pub(crate) fn policy(&self, local: Option<SocketAddr>) -> Arc<Policy> {
        let mut origins = self.allowed_origins.clone();
        let mut hosts = Vec::new();
        if let Some(local) = local {
            let port = local.port();
            let ip = match local.ip() {
                IpAddr::V4(ip) => ip.to_string(),
                IpAddr::V6(ip) => format!("[{ip}]"),
            };
            for host in [format!("{ip}:{port}"), format!("localhost:{port}")] {
                origins.push(format!("http://{host}"));
                hosts.push(host);
            }
            // A dev server proxying the API may pass its own `Host` on.
            hosts.extend(
                self.allowed_origins
                    .iter()
                    .filter_map(|origin| origin.split_once("://").map(|(_, host)| host.to_owned())),
            );
        }
        Arc::new(Policy {
            store: self.store.clone(),
            origins,
            hosts,
        })
    }
}

/// `scheme://host[:port]`, lowercased, local hosts only.
fn normalize_origin(origin: &str) -> anyhow::Result<String> {
    let normalized = origin.trim().trim_end_matches('/').to_ascii_lowercase();
    let local = normalized
        .split_once("://")
        .filter(|(scheme, _)| matches!(*scheme, "http" | "https"))
        .is_some_and(|(_, authority)| {
            !authority.contains(['/', '?', '#', '@']) && is_local_host(host_of(authority))
        });
    if !local {
        bail!(
            "allowed origin `{origin}` must look like `http://localhost:5173`: \
             a scheme and a host on this machine, without a path"
        );
    }
    Ok(normalized)
}

/// What a router enforces; see the module docs.
pub(crate) struct Policy {
    store: Option<Arc<SessionStore>>,
    /// Exact `Origin` values that may use a session.
    origins: Vec<String>,
    /// Exact `Host` values; empty when the bound address is unknown.
    hosts: Vec<String>,
}

impl Policy {
    fn identify(&self, headers: &HeaderMap) -> Result<Identity, Denied> {
        let Some(store) = &self.store else {
            return Ok(Identity::DEV);
        };
        if !self.hosts.is_empty() && !header_in(headers, HOST, &self.hosts) {
            return Err(Denied::Forbidden);
        }
        // Browsers send `Origin` on every cross-origin request and every
        // WebSocket handshake; same-origin GETs may omit it.
        if headers.contains_key(ORIGIN) && !header_in(headers, ORIGIN, &self.origins) {
            return Err(Denied::Forbidden);
        }
        session_cookies(headers)
            .find_map(|value| store.identify(value))
            .ok_or(Denied::Unauthenticated)
    }
}

fn header_in(headers: &HeaderMap, name: axum::http::HeaderName, allowed: &[String]) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| allowed.iter().any(|ok| ok.eq_ignore_ascii_case(value)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Denied {
    Unauthenticated,
    Forbidden,
}

/// Route layer for the API: inserts the caller's [`Identity`] or answers
/// `401` (no valid session) / `403` (not an allowed origin). Public routes
/// pass.
pub(crate) async fn require_identity(
    State(policy): State<Arc<Policy>>,
    mut request: Request,
    next: Next,
) -> Response {
    let public = request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|matched| PUBLIC_ROUTES.contains(&matched.as_str()));
    if public {
        return next.run(request).await;
    }
    match policy.identify(request.headers()) {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(Denied::Unauthenticated) => StatusCode::UNAUTHORIZED.into_response(),
        Err(Denied::Forbidden) => {
            tracing::debug!("refused a session from an origin that is not allowed");
            StatusCode::FORBIDDEN.into_response()
        }
    }
}

/// `GET /api/v1/session?code=…`.
pub(crate) fn routes(policy: Arc<Policy>) -> Router {
    Router::new()
        .route(LOGIN_PATH, get(redeem))
        .with_state(policy)
}

async fn redeem(State(policy): State<Arc<Policy>>, RawQuery(query): RawQuery) -> Response {
    let Some(store) = &policy.store else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let session = query
        .as_deref()
        .and_then(code_param)
        .and_then(|code| store.redeem_at(code, Instant::now()));
    let Some(session) = session else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let cookie = format!("{SESSION_COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/");
    let Ok(cookie) = HeaderValue::from_str(&cookie) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    (
        StatusCode::SEE_OTHER,
        [
            (SET_COOKIE, cookie),
            (LOCATION, HeaderValue::from_static("/")),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
        ],
    )
        .into_response()
}

fn code_param(query: &str) -> Option<&str> {
    query.split('&').find_map(|pair| pair.strip_prefix("code="))
}

fn session_cookies(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().strip_prefix(SESSION_COOKIE)?.strip_prefix('='))
}

fn random<const N: usize>() -> anyhow::Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("the system random generator failed: {error}"))?;
    Ok(bytes)
}

fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    // Longest valid base64url text for N bytes; anything longer is garbage.
    if text.len() > N.div_ceil(3) * 4 {
        return None;
    }
    URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()
}

struct PendingCode {
    secret: [u8; LOGIN_CODE_BYTES],
    expires: Instant,
}

struct Session {
    id: [u8; SESSION_ID_BYTES],
    identity: Identity,
}

#[derive(Default)]
struct Inner {
    codes: VecDeque<PendingCode>,
    sessions: VecDeque<Session>,
}

/// Login codes and sessions, in memory only: a restart signs everyone out
/// and the shell signs the panel in again.
#[derive(Default)]
pub(crate) struct SessionStore {
    inner: Mutex<Inner>,
}

impl SessionStore {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn issue_code_at(&self, now: Instant) -> anyhow::Result<LoginCode> {
        let secret = random::<LOGIN_CODE_BYTES>()?;
        let mut inner = self.lock();
        inner.codes.retain(|code| code.expires > now);
        while inner.codes.len() >= MAX_PENDING_CODES {
            inner.codes.pop_front();
        }
        inner.codes.push_back(PendingCode {
            secret,
            expires: now + LOGIN_CODE_TTL,
        });
        Ok(LoginCode {
            code: URL_SAFE_NO_PAD.encode(secret),
        })
    }

    /// Redeems `code` once; returns the new session id (base64url).
    fn redeem_at(&self, code: &str, now: Instant) -> Option<String> {
        let secret = decode::<LOGIN_CODE_BYTES>(code)?;
        let mut inner = self.lock();
        inner.codes.retain(|code| code.expires > now);
        // Compare against every pending code, without stopping early.
        let mut found = None;
        for (index, pending) in inner.codes.iter().enumerate() {
            if bool::from(pending.secret.ct_eq(&secret)) {
                found = Some(index);
            }
        }
        inner.codes.remove(found?);
        let id = match random::<SESSION_ID_BYTES>() {
            Ok(id) => id,
            Err(error) => {
                tracing::error!(%error, "could not create a session");
                return None;
            }
        };
        while inner.sessions.len() >= MAX_SESSIONS {
            inner.sessions.pop_front();
        }
        inner.sessions.push_back(Session {
            id,
            identity: Identity::LOCAL_PANEL,
        });
        Some(URL_SAFE_NO_PAD.encode(id))
    }

    fn identify(&self, session: &str) -> Option<Identity> {
        let id = decode::<SESSION_ID_BYTES>(session)?;
        let inner = self.lock();
        let mut identity = None;
        for session in &inner.sessions {
            if bool::from(session.id.ct_eq(&id)) {
                identity = Some(session.identity);
            }
        }
        identity
    }
}

#[cfg(test)]
mod tests {
    use axum::Extension;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware;
    use tower::ServiceExt;

    use super::*;

    fn session_of(store: &SessionStore, now: Instant) -> String {
        let code = store.issue_code_at(now).unwrap();
        store.redeem_at(code.as_str(), now).unwrap()
    }

    fn sessions(allowed: &[&str]) -> Auth {
        let allowed: Vec<String> = allowed.iter().map(|&o| o.to_owned()).collect();
        Auth::new(ApiAuth::Sessions, &allowed).unwrap()
    }

    #[test]
    fn login_codes_are_random_128_bit_base64url() {
        let store = SessionStore::default();
        let now = Instant::now();
        let a = store.issue_code_at(now).unwrap();
        let b = store.issue_code_at(now).unwrap();
        assert_ne!(a.as_str(), b.as_str());
        assert_eq!(a.as_str().len(), 22, "16 bytes, base64url without padding");
        assert!(
            a.as_str()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert_eq!(
            a.login_path(),
            format!("/api/v1/session?code={}", a.as_str())
        );
        assert!(!format!("{a:?}").contains(a.as_str()), "Debug must redact");
    }

    #[test]
    fn a_login_code_works_once() {
        let store = SessionStore::default();
        let now = Instant::now();
        let code = store.issue_code_at(now).unwrap();
        let session = store.redeem_at(code.as_str(), now).unwrap();
        assert_eq!(session.len(), 43, "32 bytes, base64url without padding");
        assert_eq!(store.redeem_at(code.as_str(), now), None);
        assert_eq!(store.identify(&session), Some(Identity::LOCAL_PANEL));
    }

    #[test]
    fn a_login_code_expires() {
        let store = SessionStore::default();
        let now = Instant::now();
        let late = store.issue_code_at(now).unwrap();
        let on_time = store.issue_code_at(now).unwrap();
        let almost = now + LOGIN_CODE_TTL - Duration::from_millis(1);
        assert!(store.redeem_at(on_time.as_str(), almost).is_some());
        assert_eq!(store.redeem_at(late.as_str(), now + LOGIN_CODE_TTL), None);
        // Expired codes are forgotten, not just refused.
        assert!(store.lock().codes.is_empty());
    }

    #[test]
    fn wrong_or_malformed_codes_and_sessions_are_refused() {
        let store = SessionStore::default();
        let now = Instant::now();
        let code = store.issue_code_at(now).unwrap();
        let mut wrong = code.as_str().to_owned();
        let last = if wrong.ends_with('A') { "B" } else { "A" };
        wrong.replace_range(wrong.len() - 1.., last);
        let long = "A".repeat(4096);
        for bad in ["", "x", "not base64!", wrong.as_str(), long.as_str()] {
            assert_eq!(store.redeem_at(bad, now), None, "{bad:?}");
            assert_eq!(store.identify(bad), None, "{bad:?}");
        }
        // The real code still works after the failed attempts.
        let session = store.redeem_at(code.as_str(), now).unwrap();
        assert_eq!(store.identify(&session), Some(Identity::LOCAL_PANEL));
        assert_eq!(
            store.identify(code.as_str()),
            None,
            "a code is not a session"
        );
    }

    #[test]
    fn old_codes_and_sessions_are_dropped_beyond_the_limits() {
        let store = SessionStore::default();
        let now = Instant::now();
        let first_code = store.issue_code_at(now).unwrap();
        for _ in 0..MAX_PENDING_CODES {
            store.issue_code_at(now).unwrap();
        }
        assert_eq!(store.redeem_at(first_code.as_str(), now), None);

        let first_session = session_of(&store, now);
        for _ in 0..MAX_SESSIONS {
            session_of(&store, now);
        }
        assert_eq!(store.identify(&first_session), None);
        assert_eq!(store.lock().sessions.len(), MAX_SESSIONS);
    }

    #[test]
    fn reads_the_session_cookie_among_others() {
        let mut headers = HeaderMap::new();
        headers.append(
            COOKIE,
            HeaderValue::from_static("theme=dark; vesta_session=abc; vesta_session_old=x"),
        );
        headers.append(COOKIE, HeaderValue::from_static("vesta_session=def"));
        assert_eq!(
            session_cookies(&headers).collect::<Vec<_>>(),
            ["abc", "def"]
        );
    }

    #[test]
    fn allowed_origins_must_be_exact_local_origins() {
        let auth = sessions(&["http://LOCALHOST:1420/", "https://127.0.0.1:8443"]);
        assert_eq!(
            auth.allowed_origins,
            ["http://localhost:1420", "https://127.0.0.1:8443"]
        );
        for bad in [
            "localhost:1420",
            "ftp://localhost:1420",
            "http://example.com",
            "http://192.0.2.10:5173",
            "http://localhost:1420/app",
            "http://user@localhost:1420",
            "http://evil.localhost:1420",
            "",
        ] {
            assert!(
                Auth::new(ApiAuth::Sessions, &[bad.to_owned()]).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn policy_knows_the_hubs_own_origins() {
        let auth = sessions(&["http://localhost:1420"]);
        let policy = auth.policy(Some("127.0.0.1:8181".parse().unwrap()));
        assert_eq!(
            policy.origins,
            [
                "http://localhost:1420",
                "http://127.0.0.1:8181",
                "http://localhost:8181"
            ]
        );
        assert_eq!(
            policy.hosts,
            ["127.0.0.1:8181", "localhost:8181", "localhost:1420"]
        );
        let v6 = auth.policy(Some("[::1]:9000".parse().unwrap()));
        assert!(v6.origins.contains(&"http://[::1]:9000".to_owned()));
        let unbound = auth.policy(None);
        assert_eq!(unbound.origins, ["http://localhost:1420"]);
        assert!(unbound.hosts.is_empty());
    }

    #[test]
    fn sessions_only_from_the_exact_allowed_origins() {
        let auth = sessions(&["http://localhost:1420"]);
        let store = auth.store.clone().unwrap();
        let session = session_of(&store, Instant::now());
        let policy = auth.policy(Some("127.0.0.1:8181".parse().unwrap()));
        let check = |host: &'static str, origin: Option<&'static str>| {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, HeaderValue::from_static(host));
            if let Some(origin) = origin {
                headers.insert(ORIGIN, HeaderValue::from_static(origin));
            }
            headers.insert(
                COOKIE,
                HeaderValue::from_str(&format!("{SESSION_COOKIE}={session}")).unwrap(),
            );
            policy.identify(&headers)
        };
        let ok = Ok(Identity::LOCAL_PANEL);
        assert_eq!(check("127.0.0.1:8181", None), ok, "same-origin GET");
        assert_eq!(check("127.0.0.1:8181", Some("http://127.0.0.1:8181")), ok);
        assert_eq!(check("localhost:8181", Some("http://localhost:8181")), ok);
        assert_eq!(
            check("127.0.0.1:8181", Some("http://localhost:1420")),
            ok,
            "configured dev server"
        );
        // Other local ports are other sites, whatever the cookie says.
        for origin in [
            "http://127.0.0.1:3000",
            "http://localhost:5173",
            "https://127.0.0.1:8181",
            "http://127.0.0.1:81810",
            "null",
        ] {
            assert_eq!(
                check("127.0.0.1:8181", Some(origin)),
                Err(Denied::Forbidden),
                "{origin}"
            );
        }
        for host in ["127.0.0.1:3000", "localhost", "127.0.0.2:8181"] {
            assert_eq!(check(host, None), Err(Denied::Forbidden), "{host}");
        }
    }

    async fn whoami(Extension(identity): Extension<Identity>) -> String {
        format!("{identity:?}")
    }

    fn app(policy: Arc<Policy>) -> Router {
        Router::new()
            .route("/api/v1/health", get(|| async { "ok" }))
            .route("/api/v1/whoami", get(whoami))
            .route_layer(middleware::from_fn_with_state(policy, require_identity))
    }

    async fn call(router: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, String) {
        let mut request = Request::builder().uri(path).header(HOST, "127.0.0.1:8080");
        if let Some(cookie) = cookie {
            request = request.header(COOKIE, format!("{SESSION_COOKIE}={cookie}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn handlers_receive_the_identity() {
        let auth = sessions(&[]);
        let session = session_of(auth.store.as_ref().unwrap(), Instant::now());
        let router = app(auth.policy(Some("127.0.0.1:8080".parse().unwrap())));

        assert_eq!(
            call(&router, "/api/v1/health", None).await.0,
            StatusCode::OK
        );
        assert_eq!(
            call(&router, "/api/v1/whoami", None).await.0,
            StatusCode::UNAUTHORIZED
        );
        let (status, body) = call(&router, "/api/v1/whoami", Some(&session)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, format!("{:?}", Identity::LOCAL_PANEL));

        let disabled = Auth::new(ApiAuth::Disabled, &[]).unwrap();
        let (status, body) = call(&app(disabled.policy(None)), "/api/v1/whoami", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, format!("{:?}", Identity::DEV));
    }

    #[test]
    fn sessions_are_the_default() {
        assert_eq!(ApiAuth::default(), ApiAuth::Sessions);
        assert!(
            Auth::new(ApiAuth::default(), &[])
                .unwrap()
                .login_codes()
                .is_some()
        );
        assert!(
            Auth::new(ApiAuth::Disabled, &[])
                .unwrap()
                .login_codes()
                .is_none()
        );
    }
}
