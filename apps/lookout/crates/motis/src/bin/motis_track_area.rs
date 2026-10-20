use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::{Country, Root};
use medallion_model::Area;
use motis::capture::Coverage;
use motis::polling::{PollArgs, Poller};
use transport::countries::CountryAreas;
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
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_track_area=info,motis=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args
        .poll
        .medallion
        .root()
        .expect("locate the medallion store");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let (area, mut coverage) = fixed_area(&root, args.country, args.region.as_ref()).await;
    Poller::start(&args.poll, &root, Some(area))
        .await
        .expect("start polling Motis")
        .run(&mut coverage)
        .await;
}

async fn fixed_area(
    root: &Root,
    country: Country,
    region: Option<&RegionCode>,
) -> (Area, Coverage) {
    let areas = CountryAreas::newest_per_country(root)
        .await
        .expect("read the country areas from the Overture extracts");
    let country_shapes = areas.shapes_of(country);
    let (region, release, area_shapes) = match region {
        Some(code) => {
            let found = region_area(root, country, code)
                .await
                .expect("find the region in the country's newest Overture extract");
            (Some(found.region), found.release, found.shapes)
        }
        None => {
            let release = areas
                .release_of(country)
                .expect("the store holds an Overture extract of the country to capture");
            (None, release, country_shapes.clone())
        }
    };
    let coverage = Coverage::area(area_shapes, country_shapes)
        .expect("the newest Overture extract holds a shape for the area to capture");
    tracing::info!(
        country = country.code(),
        region = region.as_ref().map(|region| region.name.as_str()),
        %release,
        "capturing a fixed area"
    );
    (
        Area {
            country,
            overture_region: region,
            overture_release: release,
        },
        coverage,
    )
}
