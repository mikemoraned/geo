use axum::body::Body;
use axum::http::{StatusCode, header};
use server::auth::Allowing;
use server::auth::store::AuthStore;

use crate::support::{RECORD_PATHS, Reply, Server, get, post, signed_in_with_code};

fn assert_sent_to_signin(path: &str, reply: &Reply) {
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "{path}: {:?}",
        reply.body
    );
    assert_eq!(
        reply.headers.get(header::LOCATION).map(|to| to.as_bytes()),
        Some(b"/signin/".as_slice()),
        "{path}"
    );
}

#[tokio::test]
async fn the_record_page_sends_a_visitor_without_a_session_to_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in RECORD_PATHS {
        assert_sent_to_signin(path, &get(&server.app, path, None).await);
    }
}

#[tokio::test]
async fn no_other_spelling_of_a_record_path_reaches_the_record_page() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in [
        "//record/",
        "//record/app.js",
        "/record%2Fapp.js",
        "/./record/index.html",
    ] {
        let reply = get(&server.app, path, None).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_signed_in_session_reaches_the_record_page() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    for path in RECORD_PATHS {
        let reply = get(&server.app, path, Some(&cookie)).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {:?}", reply.body);
    }
}

#[tokio::test]
async fn revoking_a_passkey_sends_only_its_session_to_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (revoked, shut_out) = signed_in_with_code(&server).await;
    let (_, kept) = signed_in_with_code(&server).await;

    server
        .passkeys
        .remove_allowed(&revoked)
        .await
        .expect("revoked");

    for path in RECORD_PATHS {
        assert_sent_to_signin(path, &get(&server.app, path, Some(&shut_out)).await);
        let reply = get(&server.app, path, Some(&kept)).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {:?}", reply.body);
    }
}

#[tokio::test]
async fn signing_out_sends_the_session_to_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    post(&server.app, "/auth/signout", Body::empty(), Some(&cookie)).await;

    for path in RECORD_PATHS {
        assert_sent_to_signin(path, &get(&server.app, path, Some(&cookie)).await);
    }
}

#[tokio::test]
async fn the_other_pages_need_no_session() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in ["/", "/live/", "/kiosk/", "/signin/"] {
        let reply = get(&server.app, path, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {:?}", reply.body);
    }
}
