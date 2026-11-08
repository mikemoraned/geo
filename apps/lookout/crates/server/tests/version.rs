use axum::body::Body;
use axum::http::{Request, StatusCode};
use server::auth::RelyingParty;
use server::{AppState, GIT_HASH, build_app};
use tower::ServiceExt;
use webauthn_rs::prelude::Url;

fn relying_party() -> RelyingParty {
    let origin = Url::parse("https://lookout.example.com").expect("an origin");
    RelyingParty::in_memory(&origin).expect("a relying party")
}

#[tokio::test]
async fn version_endpoint_serves_git_hash() {
    let state = AppState {
        sink: None,
        relying_party: relying_party(),
    };
    let app = build_app(state, env!("CARGO_MANIFEST_DIR"));

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
