//! Integration test for the `/version` endpoint: it serves the build's git hash
//! (`GIT_HASH`) so a running deploy can be matched to its source. Driven through the
//! real router via `oneshot`, no network needed.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use server::{AppState, GIT_HASH, build_app};
use tower::ServiceExt; // for `oneshot`

fn static_dir() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/static").to_string()
}

#[tokio::test]
async fn version_endpoint_serves_git_hash() {
    let app = build_app(AppState { sink: None }, static_dir());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/version")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    assert_eq!(body, GIT_HASH.as_bytes());
    assert!(!GIT_HASH.is_empty());
}
