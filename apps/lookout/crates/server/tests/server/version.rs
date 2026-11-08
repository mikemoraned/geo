use axum::http::StatusCode;
use server::GIT_HASH;
use server::auth::Allowing;

use crate::support::{Server, get};

#[tokio::test]
async fn version_endpoint_serves_git_hash() {
    let server = Server::hosted(Allowing::ByAdmin);

    let reply = get(&server.app, "/version", None).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, GIT_HASH.as_bytes());
    assert!(!GIT_HASH.is_empty());
}
