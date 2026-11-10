use std::fmt;

use redis::aio::MultiplexedConnection;

pub const REDIS_URL_VAR: &str = "LOOKOUT_REDIS_URL";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Store {
    Redis,
    InMemory,
}

pub fn fatal(reason: &dyn fmt::Display) -> ! {
    eprintln!("FATAL: {reason}");
    std::process::exit(1);
}

pub async fn connect_or_exit(url: &str) -> MultiplexedConnection {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");
    telemetry::connect(url)
        .await
        .unwrap_or_else(|err| fatal(&format!("redis connect failed: {err}")))
}
