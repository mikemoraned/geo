use clap::Parser;
use tracing_subscriber::EnvFilter;

use domain::Span;
use medallion::MedallionArgs;
use recorder::sessions::{Gap, Lead, sessions};
use recorder::silver;
use transport::countries::CountryAreas;

#[derive(Parser)]
#[command(about = "Derive the silver session datasets from the bronze telemetry")]
struct Args {
    /// How long a device may go unheard before the silence separates two sessions, such as 10m.
    #[arg(long, default_value = "10m")]
    gap: Span,
    /// How long before reporting a session a device may already have been fixing its
    /// position, such as 1m. Samples this close ahead of a report open the session it reports.
    #[arg(long, default_value = "1m")]
    lead: Span,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "sessionise=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");
    let gap = Gap::new(args.gap.age());
    let lead = Lead::new(args.lead.age());

    let countries = CountryAreas::newest_per_country(&root)
        .await
        .expect("read the country areas of the newest extract");
    let derived = sessions(&root, gap, lead).await.expect("derive sessions");
    let outcome = silver::write(&root, &derived, &countries)
        .await
        .expect("write sessions");

    tracing::info!(
        sessions = outcome.sessions,
        samples = outcome.samples,
        session_partitions = outcome.session_partitions.written,
        session_partitions_removed = outcome.session_partitions.removed,
        sample_partitions = outcome.sample_partitions.written,
        sample_partitions_removed = outcome.sample_partitions.removed,
        unplaceable = outcome.unplaceable,
        gap = %args.gap,
        lead = %args.lead,
        medallion_root = %root.path().display(),
        "derived sessions"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flags_default_to_the_default_gap_and_lead() {
        let args = Args::parse_from(["sessionise"]);

        assert_eq!(Gap::new(args.gap.age()), Gap::default());
        assert_eq!(Lead::new(args.lead.age()), Lead::default());
    }

    #[test]
    fn a_gap_and_a_lead_are_written_as_spans() {
        let args = Args::parse_from(["sessionise", "--gap", "2h", "--lead", "30s"]);

        assert_eq!(args.gap.to_string(), "2h");
        assert_eq!(args.lead.to_string(), "30s");
    }

    #[test]
    fn a_gap_or_a_lead_of_zero_is_refused() {
        for flag in ["--gap", "--lead"] {
            assert!(
                Args::try_parse_from(["sessionise", flag, "0s"]).is_err(),
                "{flag}"
            );
        }
    }
}
