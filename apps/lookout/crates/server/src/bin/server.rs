use std::net::SocketAddr;
use std::sync::Arc;

use server::queue::RedisSink;
use server::{build_app, AppState};

fn static_dir() -> String {
    std::env::var("LOOKOUT_STATIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/static").to_string())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=info,tower_http=info".into()),
        )
        .init();

    // rustls needs a process-global crypto provider installed once before any
    // `rediss://` (TLS) connection is made.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let sink = match std::env::var("LOOKOUT_REDIS_URL") {
        Ok(url) => match RedisSink::connect(&url).await {
            Ok(sink) => {
                tracing::info!("connected to telemetry redis");
                Some(Arc::new(sink) as Arc<dyn server::queue::SampleSink>)
            }
            Err(err) => {
                tracing::error!(%err, "failed to connect to telemetry redis; running log-only");
                None
            }
        },
        Err(_) => {
            tracing::warn!("LOOKOUT_REDIS_URL unset; received samples will be logged only");
            None
        }
    };

    let app = build_app(AppState { sink }, static_dir());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
