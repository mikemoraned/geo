use chrono::Utc;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use motis::landing_to_bronze_promotion::promote;

#[derive(Parser)]
#[command(about = "Move Motis polls from landing into bronze")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_promote_landing_to_bronze=info".into()),
        )
        .init();

    let root = Args::parse().medallion.root()?;
    let promoted = promote(&root, Utc::now()).await?;
    tracing::info!(
        files = promoted.files,
        segments = promoted.segments,
        polylines = promoted.polylines,
        "promoted landing into bronze"
    );
    Ok(())
}
