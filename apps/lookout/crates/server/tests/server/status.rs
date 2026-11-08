use axum::Router;
use axum::body::Body;
use axum::http::StatusCode;
use server::auth::Allowing;
use server::auth::status::SignInStatus;
use server::auth::store::AuthStore;

use tower_sessions::cookie::Cookie;

use crate::support::{Server, get, post, signed_in_with_code};

async fn sign_in_status(app: &Router, cookie: Option<&Cookie<'_>>) -> SignInStatus {
    let reply = get(app, "/auth/status", cookie).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    serde_json::from_slice(&reply.body).expect("a status")
}

#[tokio::test]
async fn the_status_without_a_session_is_signed_out() {
    let server = Server::hosted(Allowing::ByAdmin);

    assert_eq!(
        sign_in_status(&server.app, None).await,
        SignInStatus::SignedOut
    );
}

#[tokio::test]
async fn the_status_of_a_session_with_an_allowed_passkey_is_signed_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    assert_eq!(
        sign_in_status(&server.app, Some(&cookie)).await,
        SignInStatus::SignedIn
    );
}

#[tokio::test]
async fn revoking_a_passkey_signs_out_only_its_session() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (revoked, shut_out) = signed_in_with_code(&server).await;
    let (_, kept) = signed_in_with_code(&server).await;

    server
        .passkeys
        .remove_allowed(&revoked)
        .await
        .expect("revoked");

    assert_eq!(
        (
            sign_in_status(&server.app, Some(&shut_out)).await,
            sign_in_status(&server.app, Some(&kept)).await,
        ),
        (SignInStatus::SignedOut, SignInStatus::SignedIn)
    );
}

#[tokio::test]
async fn the_status_after_signing_out_is_signed_out() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    post(&server.app, "/auth/signout", Body::empty(), Some(&cookie)).await;

    assert_eq!(
        sign_in_status(&server.app, Some(&cookie)).await,
        SignInStatus::SignedOut
    );
}
