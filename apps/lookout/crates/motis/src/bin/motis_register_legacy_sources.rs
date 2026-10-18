use chrono::Utc;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use motis::legacy::legacy_sources;
use motis::source::register;

#[derive(Parser)]
#[command(
    about = "Record the sources of the motis_segment captures, which predate recorded sources"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_register_legacy_sources=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    for source in legacy_sources() {
        let registration = register(&root, &source, Utc::now())
            .await
            .expect("record the legacy source");
        tracing::info!(
            source_id = %source.id(),
            label = source.label.as_ref().map(|label| label.to_string()),
            ?registration,
            "recorded a legacy source"
        );
    }
}
