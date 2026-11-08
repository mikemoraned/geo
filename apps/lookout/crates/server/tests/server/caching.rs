use axum::body::Body;
use axum::http::header;
use server::auth::Allowing;

use crate::support::{
    Browser, ORIGIN, RECORD_PATHS, Server, get, post, register, sign_in, signed_in_with_code,
};

#[tokio::test]
async fn a_browser_asks_the_server_before_reusing_the_record_page() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    for path in RECORD_PATHS {
        let reply = get(&server.app, path, Some(&cookie)).await;
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .map(|value| value.as_bytes()),
            Some(&b"private, no-cache"[..]),
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_browser_asks_the_server_before_reusing_any_other_file() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in [
        "/",
        "/signin/",
        "/signin/signin.js",
        "/version.js",
        "/sessions.json",
    ] {
        let reply = get(&server.app, path, None).await;
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .map(|value| value.as_bytes()),
            Some(&b"no-cache"[..]),
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_browser_never_stores_an_auth_answer() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let signed_in = sign_in(&server.app, &mut browser, None).await;
    let cookie = signed_in.session_cookie().expect("a session");

    let replies = [
        ("a sign-in", signed_in),
        (
            "a registration's start",
            post(&server.app, "/auth/register/start", Body::empty(), None).await,
        ),
        (
            "a sign-in's start",
            post(&server.app, "/auth/signin/start", Body::empty(), None).await,
        ),
        (
            "a refused finish",
            post(&server.app, "/auth/signin/finish", Body::from("{}"), None).await,
        ),
        (
            "the status",
            get(&server.app, "/auth/status", Some(&cookie)).await,
        ),
        (
            "a sign-out",
            post(&server.app, "/auth/signout", Body::empty(), Some(&cookie)).await,
        ),
    ];

    for (answer, reply) in replies {
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .map(|value| value.as_bytes()),
            Some(&b"no-store"[..]),
            "{answer}"
        );
    }
}

#[tokio::test]
async fn an_answer_from_a_route_that_sets_no_cache_control_is_never_stored() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in ["/version", "/record/"] {
        let reply = get(&server.app, path, None).await;
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .map(|value| value.as_bytes()),
            Some(&b"no-store"[..]),
            "{path}"
        );
    }
}
