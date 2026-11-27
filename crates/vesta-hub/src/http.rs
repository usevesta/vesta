//! The full local endpoint: API routes behind the session layer, the
//! login route, the UI (when the embedder provides one) and the local-only
//! guard in front of everything.

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::HeaderValue;
use axum::http::header::{REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::middleware;
use axum::response::Response;

use crate::api;
use crate::assets::UiService;
use crate::auth::{self, Policy};

/// `api` is the router from [`api::router`]: every route in it needs an
/// identity unless it is listed as public in [`auth`].
pub(crate) fn app(api: Router, policy: Arc<Policy>, ui: Option<UiService>) -> Router {
    let api = api.route_layer(middleware::from_fn_with_state(
        Arc::clone(&policy),
        auth::require_identity,
    ));
    let mut app = api.merge(auth::routes(policy));
    if let Some(ui) = ui {
        app = app.fallback(move |request: Request| {
            let ui = ui.clone();
            async move { ui.respond(request.method(), request.uri()) }
        });
    }
    // The API routes already carry the local-only guard; this covers the
    // login route and the UI too.
    app.layer(middleware::map_response(security_headers))
        .layer(middleware::from_fn(api::local_only))
}

async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
}
