use clap::Parser;

use server::auth::admin::{Command, run};
use server::auth::redis::RedisAuthStore;
use server::stores::REDIS_URL_VAR;

#[derive(Parser)]
#[command(about = "List, allow and revoke the passkeys that can sign in to the lookout site")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

fn fatal(reason: &dyn std::fmt::Display) -> ! {
    eprintln!("FATAL: {reason}");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let url = std::env::var(REDIS_URL_VAR)
        .unwrap_or_else(|_| fatal(&format!("{REDIS_URL_VAR} must be set")));
    let conn = telemetry::connect(&url)
        .await
        .unwrap_or_else(|err| fatal(&format!("redis connect failed: {err}")));
    match run(&RedisAuthStore::new(conn), args.command).await {
        Ok(said) => println!("{said}"),
        Err(err) => fatal(&err),
    }
}
