use clap::Parser;

use server::auth::admin::{Command, run};
use server::auth::redis::RedisAuthStore;
use server::stores::{REDIS_URL_VAR, connect_or_exit, fatal};

#[derive(Parser)]
#[command(about = "List, allow and revoke the passkeys that can sign in to the lookout site")]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// The redis URL of the store that holds the passkeys.
    #[arg(long, env = REDIS_URL_VAR, hide_env_values = true)]
    redis_url: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let conn = connect_or_exit(&args.redis_url).await;
    match run(&RedisAuthStore::new(conn), args.command).await {
        Ok(said) => println!("{said}"),
        Err(err) => fatal(&err),
    }
}
