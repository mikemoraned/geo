pub mod auth;
pub mod queue;
pub mod stores;
mod ws;

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::extract::FromRef;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::routing::{any, get};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::{SetResponseHeader, SetResponseHeaderLayer};
use tower_http::trace::TraceLayer;

use crate::auth::RelyingParty;
use crate::queue::SampleSink;

pub const GIT_HASH: &str = env!("BUILD_GIT_HASH");

#[derive(Clone)]
pub struct AppState {
    pub sink: Option<Arc<dyn SampleSink>>,
    pub relying_party: RelyingParty,
}

impl FromRef<AppState> for RelyingParty {
    fn from_ref(state: &AppState) -> Self {
        state.relying_party.clone()
    }
}

const RECORD_PAGE: &str = "/record";
const ASK_FIRST: &str = "no-cache";
const ASK_FIRST_PRIVATELY: &str = "private, no-cache";
const NEVER_STORE: &str = "no-store";

pub fn build_app(state: AppState, site_dir: impl AsRef<Path>) -> Router {
    let site_dir = site_dir.as_ref();
    let record = Router::new()
        .nest_service(
            RECORD_PAGE,
            SetResponseHeader::overriding(
                ServeDir::new(site_dir.join("gated/record")),
                header::CACHE_CONTROL,
                HeaderValue::from_static(ASK_FIRST_PRIVATELY),
            ),
        )
        .layer(middleware::from_fn_with_state(
            state.relying_party.clone(),
            auth::gate::require_signin,
        ));
    let with_sessions = Router::new()
        .route("/ws", any(ws::upgrade))
        .merge(auth::routes(state.relying_party.clone()))
        .merge(record)
        .layer(state.relying_party.session_layer());
    Router::new()
        .route("/version", get(version))
        .merge(with_sessions)
        .fallback_service(SetResponseHeader::overriding(
            ServeDir::new(site_dir.join("static")),
            header::CACHE_CONTROL,
            HeaderValue::from_static(ASK_FIRST),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static(NEVER_STORE),
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn version() -> &'static str {
    GIT_HASH
}
