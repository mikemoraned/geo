//! `summarise`: report what a medallion store currently holds — every dataset defined for
//! this app, in every layer, plus the gold artefacts.
//!
//! Reads no rows: the counts come from each parquet file's own footer, so this stays cheap
//! on a store far too large to scan. A dataset nothing has written is reported as absent
//! rather than left out, since what is missing is half of what the question is asking.

use clap::Parser;
use medallion::MedallionArgs;
use summary::{Detail, report};

#[derive(Parser)]
#[command(about = "Summarise what the medallion store holds")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// List every partition of every dataset, rather than the span each one covers.
    #[arg(long)]
    partitions: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let root = args.medallion.root()?;

    let datasets = model::ALL
        .into_iter()
        .map(|dataset| medallion::summary::dataset(&root, dataset))
        .collect::<Result<Vec<_>, _>>()?;
    let artefacts = medallion::summary::artefacts(&root)?;
    let detail = match args.partitions {
        true => Detail::Partitions,
        false => Detail::Datasets,
    };

    println!("{}", root.path().display());
    print!("{}", report(&datasets, &artefacts, detail));
    Ok(())
}
