use std::fs;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tempfile::TempDir;

use crate::support::{Reply, Server, send};

fn site_with_tiles() -> TempDir {
    let site = tempfile::tempdir().expect("a temporary site dir");
    let tiles: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    fs::create_dir(site.path().join("static")).expect("a static dir");
    fs::write(site.path().join("static/map.pmtiles"), tiles).expect("a tiles file");
    site
}

async fn fetch_range(range: &str) -> Reply {
    let site = site_with_tiles();
    let server = Server::hosting(site.path());
    let request = Request::builder()
        .uri("/map.pmtiles")
        .header(header::RANGE, range)
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .expect("build request");
    send(&server.app, request).await
}

#[tokio::test]
async fn a_range_of_tiles_is_served_as_those_bytes() {
    let reply = fetch_range("bytes=16-23").await;

    assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        reply.headers.get(header::CONTENT_RANGE).map(|value| value.as_bytes()),
        Some(b"bytes 16-23/4096".as_slice()),
    );
    assert_eq!(reply.body.as_ref(), [16, 17, 18, 19, 20, 21, 22, 23]);
}

#[tokio::test]
async fn a_range_of_tiles_is_served_uncompressed_to_a_client_that_takes_gzip() {
    let reply = fetch_range("bytes=0-1023").await;

    assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(reply.headers.get(header::CONTENT_ENCODING), None);
    assert_eq!(reply.body.len(), 1024);
}
