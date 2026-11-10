use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use server::auth::Allowing;

use crate::support::{Server, send};

async fn fetch(path: &str, accept_encoding: Option<&str>) -> (StatusCode, Option<String>, usize) {
    let mut request = Request::builder().uri(path);
    if let Some(encoding) = accept_encoding {
        request = request.header(header::ACCEPT_ENCODING, encoding);
    }
    let server = Server::hosted(Allowing::ByAdmin);
    let reply = send(
        &server.app,
        request.body(Body::empty()).expect("build request"),
    )
    .await;

    let encoding = reply
        .headers
        .get(header::CONTENT_ENCODING)
        .map(|value| value.to_str().expect("an ascii encoding").to_string());

    (reply.status, encoding, reply.body.len())
}

#[tokio::test]
async fn a_client_that_takes_gzip_is_served_gzip() {
    let (status, encoding, _) = fetch("/index.html", Some("gzip")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(encoding.as_deref(), Some("gzip"));
}

#[tokio::test]
async fn a_client_that_asks_for_nothing_is_served_as_it_is() {
    let (status, encoding, _) = fetch("/index.html", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(encoding, None);
}

#[tokio::test]
async fn the_crossings_are_worth_compressing() {
    let (status, _, plain) = fetch("/crossings.json", None).await;
    if status == StatusCode::NOT_FOUND {
        eprintln!("no crossings.json in static");
        return;
    }

    let (status, encoding, compressed) = fetch("/crossings.json", Some("gzip")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(encoding.as_deref(), Some("gzip"));
    assert!(
        compressed < plain / 2,
        "{compressed} bytes compressed is no saving over {plain}",
    );
}
