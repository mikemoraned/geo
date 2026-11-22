use clap::Parser;
use tracing_subscriber::EnvFilter;

use countries::CountryAreas;
use medallion::{Country, Root};
use medallion_model::Area;
use motis::capture::Coverage;
use motis::polling::{PollArgs, Poller};
use transport::regions::{RegionCode, region_area};

#[derive(Parser)]
#[command(about = "Poll Motis for every train in one country or region, and log them")]
struct Args {
    #[command(flatten)]
    poll: PollArgs,
    /// The country to capture, as an ISO 3166-1 alpha-2 code, such as DE. Its shape comes from
    /// the country's newest Overture extract.
    #[arg(long)]
    country: Country,
    /// Narrow the country to one of its principal subdivisions, named by its ISO 3166-2 code,
    /// such as DE-TH.
    #[arg(long)]
    region: Option<RegionCode>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_track_area=info,motis=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.poll.medallion.root()?;

    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "a rustls crypto provider was already installed")?;

    let (area, mut coverage) = fixed_area(&root, args.country, args.region.as_ref()).await?;
    Poller::start(&args.poll, &root, Some(area))
        .await?
        .run(&mut coverage)
        .await;
    Ok(())
}

async fn fixed_area(
    root: &Root,
    country: Country,
    region: Option<&RegionCode>,
) -> Result<(Area, Coverage), Box<dyn std::error::Error>> {
    let areas = CountryAreas::newest_per_country(root).await?;
    let extract = areas
        .newest_extract(country)
        .ok_or_else(|| format!("the store holds no Overture extract of {}", country.code()))?;
    let release = extract.release;
    let outline = areas
        .outline(country)
        .ok_or_else(|| {
            format!(
                "the newest Overture extract holds no shape for {}",
                country.code()
            )
        })?
        .clone();
    let (region, area_shapes) = match region {
        Some(code) => {
            let found = region_area(root, &extract.id, country, code).await?;
            (Some(found.region), found.shapes)
        }
        None => (None, areas.shapes_of(country)),
    };
    let coverage = Coverage::area(area_shapes, outline)
        .ok_or("the newest Overture extract holds no shape for the area to capture")?;
    tracing::info!(
        country = country.code(),
        region = region.as_ref().map(|region| region.name.as_str()),
        %release,
        "capturing a fixed area"
    );
    Ok((
        Area {
            country,
            overture_region: region,
            overture_release: release,
        },
        coverage,
    ))
}
