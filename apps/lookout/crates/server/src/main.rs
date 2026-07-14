use std::net::SocketAddr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

const STATIC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static");

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=info,tower_http=info".into()),
        )
        .init();

    let app = Router::new()
        .route("/ws", any(ws_upgrade))
        .fallback_service(ServeDir::new(STATIC_DIR))
        .layer(TraceLayer::new_for_http());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}

async fn ws_upgrade(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    tracing::info!("websocket connected");
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => tracing::info!(%text, "sample"),
            Message::Close(_) => break,
            _ => {}
        }
    }
    tracing::info!("websocket disconnected");
}
