use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;

use server::auth::RelyingParty;
use server::queue::{RedisSink, SampleSink};
use server::stores::{REDIS_URL_VAR, Store, Stores};
use server::{AppState, build_app};
use webauthn_rs::prelude::Url;

const LOCAL_ORIGIN: &str = "http://localhost:3000";

fn site_dir() -> String {
    std::env::var("LOOKOUT_SITE_DIR").unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string())
}

#[derive(Parser)]
#[command(about = "Serve the lookout site, and receive what its pages send")]
struct Args {
    /// Where to queue samples and keep passkeys and sessions. `redis` reads its URL from
    /// LOOKOUT_REDIS_URL. `in-memory` logs samples only, and forgets passkeys and sessions on
    /// restart.
    #[arg(long, value_enum)]
    store: Store,

    /// The origin the site is served from, such as `https://lookout.example.com`. Passkeys work
    /// only on the origin they were registered on.
    #[arg(long, env = "LOOKOUT_ORIGIN", default_value = LOCAL_ORIGIN)]
    origin: Url,
}

fn fatal(reason: &dyn std::fmt::Display) -> ! {
    eprintln!("FATAL: {reason}");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=info,tower_http=info".into()),
        )
        .init();

    tracing::info!(git_hash = server::GIT_HASH, "server starting");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let origin = args.origin;
    tracing::info!(%origin, "serving passkeys for this origin");
    let redis_url = std::env::var(REDIS_URL_VAR).ok();
    if args.store == Store::InMemory && redis_url.is_some() {
        tracing::warn!("ignoring {REDIS_URL_VAR}, since the store is in-memory");
    }
    let stores = Stores::choose(args.store, redis_url).unwrap_or_else(|err| fatal(&err));
    let (sink, relying_party) = match stores {
        Stores::Redis { url } => {
            let conn = telemetry::connect(&url)
                .await
                .unwrap_or_else(|err| fatal(&format!("redis connect failed: {err}")));
            tracing::info!("queuing samples in redis, and keeping passkeys and sessions there");
            let sink: Arc<dyn SampleSink> = Arc::new(RedisSink::new(conn.clone()));
            (Some(sink), RelyingParty::over_redis(&origin, conn))
        }
        Stores::InMemory => {
            tracing::warn!("logging samples only, and keeping passkeys and sessions in memory");
            (None, RelyingParty::in_memory(&origin))
        }
    };
    let relying_party = relying_party.unwrap_or_else(|err| fatal(&err));

    let app = build_app(
        AppState {
            sink,
            relying_party,
        },
        site_dir(),
    );

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!(bound = %addr, "listening on http://localhost:{port}");
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_must_be_named() {
        assert!(Args::try_parse_from(["server"]).is_err());
    }

    #[test]
    fn the_store_is_redis_or_in_memory() {
        let store = |name| Args::try_parse_from(["server", "--store", name]).map(|args| args.store);

        assert_eq!(store("redis").ok(), Some(Store::Redis));
        assert_eq!(store("in-memory").ok(), Some(Store::InMemory));
        assert!(store("valkey").is_err());
    }

    #[test]
    fn the_origin_defaults_to_localhost() {
        let args = Args::try_parse_from(["server", "--store", "in-memory"]).expect("args");

        assert_eq!(args.origin.as_str(), "http://localhost:3000/");
    }

    #[test]
    fn the_origin_can_be_named() {
        let origin = |origin| {
            Args::try_parse_from(["server", "--store", "redis", "--origin", origin])
                .map(|args| args.origin.to_string())
        };

        assert_eq!(
            origin("https://lookout-hom.fly.dev").ok().as_deref(),
            Some("https://lookout-hom.fly.dev/")
        );
        assert!(origin("lookout-hom.fly.dev").is_err());
    }
}
