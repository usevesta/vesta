//! The dashboard the Hub serves, and saving it from the UI's editor.
//!
//! - `GET /api/v1/dashboard` answers with an `ETag`: a hash of the content,
//!   so it survives restarts and identical dashboards share it.
//! - `PUT /api/v1/dashboard` replaces the whole dashboard. It requires
//!   `If-Match` (`428` without it) and answers `412` when someone else saved
//!   first, so two people editing at once never silently overwrite each
//!   other. Saves are serialized; a save is stored durably *before* it is
//!   served or announced.
//! - Every WebSocket session is told about a new revision with
//!   `dashboard_changed` (through a `watch` channel: it never lags, a slow
//!   session just gets the latest revision).
//!
//! Where a saved dashboard goes is a [`DashboardStore`]: [`JsonFileStore`]
//! for `vesta-hubd --dashboard <path>` (and later the app's data directory),
//! [`MemoryStore`] for `--dev` and tests.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use anyhow::Context;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tokio::sync::{Mutex, watch};
use vesta_core::{Dashboard, DashboardChanged};

use crate::api::AppState;
use crate::runtime::Runtime;

/// Largest accepted `PUT` body. A full dashboard is a few KiB.
pub(crate) const MAX_BODY_BYTES: usize = 256 * 1024;

/// Durable storage for a saved dashboard.
pub trait DashboardStore: Send + Sync + 'static {
    /// Stores `dashboard` so it survives a restart. Called on a blocking
    /// thread, one save at a time. On error nothing may have changed.
    fn save(&self, dashboard: &Dashboard) -> anyhow::Result<()>;
}

/// Keeps edits in memory only: a restart brings back the dashboard the Hub
/// started with (`vesta-hubd --dev`, tests).
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryStore;

impl DashboardStore for MemoryStore {
    fn save(&self, _dashboard: &Dashboard) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Writes `dashboard.json` atomically: a temporary file in the same
/// directory, flushed to disk, then renamed over the old one. A crash or a
/// power cut leaves either the old or the new dashboard, never half of one.
#[derive(Debug, Clone)]
pub struct JsonFileStore {
    path: PathBuf,
}

impl JsonFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl DashboardStore for JsonFileStore {
    fn save(&self, dashboard: &Dashboard) -> anyhow::Result<()> {
        let mut json =
            serde_json::to_vec_pretty(dashboard).context("could not encode dashboard")?;
        json.push(b'\n');
        write_atomically(&self.path, &json)
            .with_context(|| format!("could not write {}", self.path.display()))
    }
}

/// Replaces `path` with `contents` atomically (see [`JsonFileStore`]). The
/// new file keeps the old one's permissions.
pub(crate) fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = dir.join(temp_name);

    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)?;
        if let Ok(existing) = fs::metadata(path) {
            file.set_permissions(existing.permissions())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        sync_dir(dir)
    })();
    if written.is_err() {
        // Best effort: never leave a stray temporary file behind.
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Makes the rename itself durable.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// A dashboard's revision: 64-bit FNV-1a of its JSON, as 16 hex digits.
/// Not a security measure, only change detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Revision(String);

impl Revision {
    pub(crate) fn of(dashboard: &Dashboard) -> anyhow::Result<Self> {
        let mut hash = Fnv1a::default();
        serde_json::to_writer(&mut hash, dashboard).context("could not encode dashboard")?;
        Ok(Self(format!("{:016x}", hash.0)))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn etag(&self) -> HeaderValue {
        // Hex digits and quotes only: always a valid header value.
        HeaderValue::from_str(&format!("\"{}\"", self.0))
            .unwrap_or_else(|_| HeaderValue::from_static("\"\""))
    }

    /// `If-Match` with strong comparison (RFC 9110 §13.1.1): `*` or a list
    /// of entity tags. Weak or malformed tags never match.
    fn matches(&self, if_match: &str) -> bool {
        if_match.split(',').map(str::trim).any(|tag| {
            tag == "*"
                || tag
                    .strip_prefix('"')
                    .and_then(|tag| tag.strip_suffix('"'))
                    .is_some_and(|tag| tag == self.0)
        })
    }
}

struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Write for Fnv1a {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct Current {
    dashboard: Arc<Dashboard>,
    revision: Revision,
}

/// Why a save did not happen.
#[derive(Debug)]
pub(crate) enum SaveError {
    /// `If-Match` does not name the current revision.
    Stale(Revision),
    /// The store failed; nothing changed.
    Store(anyhow::Error),
}

/// The current dashboard, its revision, and the one place that changes it.
pub(crate) struct Dashboards {
    /// Never held across an `.await`.
    current: RwLock<Current>,
    store: RwLock<Arc<dyn DashboardStore>>,
    /// Serializes saves, so the revision check and the write are atomic.
    saving: Mutex<()>,
    changes: watch::Sender<Revision>,
}

impl Dashboards {
    /// Starts with `dashboard`, keeping edits in memory until
    /// [`Dashboards::set_store`] is called.
    pub(crate) fn new(dashboard: Dashboard) -> anyhow::Result<Self> {
        let revision = Revision::of(&dashboard)?;
        let (changes, _) = watch::channel(revision.clone());
        Ok(Self {
            current: RwLock::new(Current {
                dashboard: Arc::new(dashboard),
                revision,
            }),
            store: RwLock::new(Arc::new(MemoryStore)),
            saving: Mutex::new(()),
            changes,
        })
    }

    pub(crate) fn set_store(&self, store: Arc<dyn DashboardStore>) {
        *self.store.write().unwrap_or_else(PoisonError::into_inner) = store;
    }

    fn current(&self) -> Current {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// New revisions as they are saved. The current one counts as seen.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Revision> {
        self.changes.subscribe()
    }

    /// Replaces the dashboard if `if_match` names the current revision.
    /// Stores it first, then serves and announces it. Saving the same
    /// content again changes nothing (no write, no announcement).
    async fn save(
        &self,
        if_match: &str,
        dashboard: Dashboard,
    ) -> Result<(Arc<Dashboard>, Revision), SaveError> {
        let _saving = self.saving.lock().await;
        let current = self.current();
        if !current.revision.matches(if_match) {
            return Err(SaveError::Stale(current.revision));
        }
        let revision = Revision::of(&dashboard).map_err(SaveError::Store)?;
        if revision == current.revision {
            return Ok((current.dashboard, current.revision));
        }

        let dashboard = Arc::new(dashboard);
        let store = Arc::clone(&*self.store.read().unwrap_or_else(PoisonError::into_inner));
        let to_store = Arc::clone(&dashboard);
        tokio::task::spawn_blocking(move || store.save(&to_store))
            .await
            .context("dashboard store task failed")
            .and_then(|stored| stored)
            .map_err(SaveError::Store)?;

        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Current {
            dashboard: Arc::clone(&dashboard),
            revision: revision.clone(),
        };
        self.changes.send_replace(revision.clone());
        Ok((dashboard, revision))
    }
}

/// The `dashboard_changed` message for a revision.
pub(crate) fn changed_message(revision: &Revision) -> DashboardChanged {
    DashboardChanged {
        revision: revision.as_str().to_owned(),
    }
}

/// Logs targets that no configured device has. They are allowed: the
/// device may be offline or gone, and the UI renders it as unavailable.
pub(crate) fn warn_unknown_targets(dashboard: &Dashboard, runtime: &Runtime) {
    for target in dashboard.targets() {
        if !runtime.has_target(target) {
            tracing::warn!(endpoint = %target, "dashboard refers to an endpoint that does not exist");
        }
    }
}

/// Error body. `message` helps developers; the UI never shows it.
#[derive(Serialize)]
struct ApiError {
    error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn error(status: StatusCode, error: &'static str, message: Option<String>) -> Response {
    (status, Json(ApiError { error, message })).into_response()
}

fn with_revision(revision: &Revision, dashboard: &Dashboard) -> Response {
    (
        [
            (ETAG, revision.etag()),
            (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        Json(dashboard),
    )
        .into_response()
}

/// `GET /api/v1/dashboard`.
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let Current {
        dashboard,
        revision,
    } = state.dashboards.current();
    with_revision(&revision, &dashboard)
}

/// Who may change the dashboard. For now, every client that reached the
/// local API.
///
/// TODO(roles): editing becomes Admin-only once pairing and roles exist
/// (AGENTS.md rule 8). Look up the caller's role from its authenticated
/// session here and refuse Members and Guests (`Err` becomes
/// `403 Forbidden`).
fn authorize_edit(_headers: &HeaderMap) -> Result<(), NotAllowed> {
    Ok(())
}

/// The caller's role may not edit the dashboard.
struct NotAllowed;

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

/// Every `If-Match` value, joined as one list. `None` when absent.
fn if_match(headers: &HeaderMap) -> Option<String> {
    let values: Vec<&str> = headers
        .get_all(IF_MATCH)
        .iter()
        // A value that is not text cannot name a revision.
        .map(|value| value.to_str().unwrap_or(""))
        .collect();
    (!values.is_empty()).then(|| values.join(","))
}

/// `PUT /api/v1/dashboard`: see the module docs.
pub(crate) async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(NotAllowed) = authorize_edit(&headers) {
        return error(StatusCode::FORBIDDEN, "not_allowed", None);
    }
    let Some(if_match) = if_match(&headers) else {
        return error(
            StatusCode::PRECONDITION_REQUIRED,
            "if_match_required",
            Some("send If-Match with the ETag the edit started from".to_owned()),
        );
    };
    if !is_json(&headers) {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required", None);
    }
    let dashboard: Dashboard = match serde_json::from_slice(&body) {
        Ok(dashboard) => dashboard,
        Err(parse) if parse.is_data() => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_dashboard",
                Some(parse.to_string()),
            );
        }
        Err(parse) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                Some(parse.to_string()),
            );
        }
    };
    if let Err(invalid) = dashboard.validate() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_dashboard",
            Some(invalid.to_string()),
        );
    }

    match state.dashboards.save(&if_match, dashboard).await {
        Ok((dashboard, revision)) => {
            tracing::info!(revision = revision.as_str(), "dashboard saved");
            warn_unknown_targets(&dashboard, &state.runtime);
            with_revision(&revision, &dashboard)
        }
        Err(SaveError::Stale(current)) => {
            tracing::debug!("refused a dashboard save based on an old revision");
            let mut response = error(StatusCode::PRECONDITION_FAILED, "stale", None);
            response.headers_mut().insert(ETAG, current.etag());
            response
        }
        Err(SaveError::Store(store_error)) => {
            // Details (paths, OS errors) stay in the Hub's log.
            tracing::error!(error = %format!("{store_error:#}"), "could not save the dashboard");
            error(StatusCode::INTERNAL_SERVER_ERROR, "save_failed", None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const DASHBOARD: &str = include_str!("../../../dashboard.example.json");

    fn dashboard() -> Dashboard {
        Dashboard::from_json(DASHBOARD).unwrap()
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "vesta-hub-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn revisions_follow_the_content() {
        let a = Revision::of(&dashboard()).unwrap();
        assert_eq!(a, Revision::of(&dashboard()).unwrap(), "stable");
        assert_eq!(a.as_str().len(), 16);
        assert!(a.as_str().bytes().all(|b| b.is_ascii_hexdigit()));

        let mut changed = dashboard();
        changed.pages[0].title = "Inicio".to_owned();
        assert_ne!(a, Revision::of(&changed).unwrap());
        assert_eq!(a.etag().to_str().unwrap(), format!("\"{}\"", a.as_str()));
    }

    #[test]
    fn if_match_uses_strong_comparison() {
        let revision = Revision("0123456789abcdef".to_owned());
        for allowed in [
            "\"0123456789abcdef\"",
            " \"0123456789abcdef\" ",
            "\"other\", \"0123456789abcdef\"",
            "*",
        ] {
            assert!(revision.matches(allowed), "{allowed}");
        }
        for refused in [
            "",
            "0123456789abcdef",
            "W/\"0123456789abcdef\"",
            "\"0123456789ABCDEF\"",
            "\"0123456789abcde\"",
            "\"other\"",
        ] {
            assert!(!revision.matches(refused), "{refused}");
        }
    }

    #[test]
    fn writes_atomically_and_keeps_no_temporary_files() {
        let dir = TempDir::new();
        let path = dir.0.join("dashboard.json");
        let store = JsonFileStore::new(&path);
        assert_eq!(store.path(), path);

        store.save(&dashboard()).unwrap();
        assert_eq!(
            Dashboard::from_json(&fs::read_to_string(&path).unwrap()).unwrap(),
            dashboard()
        );

        let mut changed = dashboard();
        changed.pages.pop();
        store.save(&changed).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.ends_with("}\n"));
        assert_eq!(Dashboard::from_json(&written).unwrap(), changed);
        assert_eq!(dir.entries(), ["dashboard.json"]);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_alone() {
        let dir = TempDir::new();
        let path = dir.0.join("dashboard.json");
        fs::write(&path, "old").unwrap();
        // The rename cannot replace a directory: the write fails late.
        let blocked = dir.0.join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("keep"), "x").unwrap();
        assert!(write_atomically(&blocked, b"new").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(dir.entries(), ["blocked", "dashboard.json"]);

        let missing = dir.0.join("missing").join("dashboard.json");
        assert!(JsonFileStore::new(missing).save(&dashboard()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn keeps_the_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new();
        let path = dir.0.join("dashboard.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomically(&path, b"{ }").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn saves_are_checked_stored_then_announced() {
        let dashboards = Dashboards::new(dashboard()).unwrap();
        let first = dashboards.current().revision;
        let mut changes = dashboards.subscribe();
        assert!(!changes.has_changed().unwrap());

        let mut edited = dashboard();
        edited.pages[0].title = "Inicio".to_owned();
        let stale = format!("\"{}\"", "0".repeat(16));
        assert!(matches!(
            dashboards.save(&stale, edited.clone()).await,
            Err(SaveError::Stale(ref current)) if *current == first
        ));
        assert!(!changes.has_changed().unwrap());

        let (saved, second) = dashboards
            .save(&format!("\"{}\"", first.as_str()), edited.clone())
            .await
            .unwrap();
        assert_eq!(*saved, edited);
        assert_ne!(second, first);
        assert!(changes.has_changed().unwrap());
        assert_eq!(*changes.borrow_and_update(), second);

        // Saving the same content again is a no-op.
        let (_, again) = dashboards
            .save(&format!("\"{}\"", second.as_str()), edited)
            .await
            .unwrap();
        assert_eq!(again, second);
        assert!(!changes.has_changed().unwrap());
    }

    struct Failing;

    impl DashboardStore for Failing {
        fn save(&self, _dashboard: &Dashboard) -> anyhow::Result<()> {
            anyhow::bail!("disk full")
        }
    }

    #[tokio::test]
    async fn a_failed_store_changes_nothing() {
        let dashboards = Dashboards::new(dashboard()).unwrap();
        dashboards.set_store(Arc::new(Failing));
        let before = dashboards.current();
        let changes = dashboards.subscribe();
        let mut edited = dashboard();
        edited.pages.clear();
        let result = dashboards
            .save(&format!("\"{}\"", before.revision.as_str()), edited)
            .await;
        assert!(matches!(result, Err(SaveError::Store(_))));
        assert_eq!(dashboards.current().revision, before.revision);
        assert_eq!(*dashboards.current().dashboard, dashboard());
        assert!(!changes.has_changed().unwrap());
    }
}
