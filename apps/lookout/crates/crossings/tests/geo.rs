use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use crossings::PlacedCrossing;
use geo::{Contains, Intersects};
use geo_types::{Geometry, Point};
use geojson::{FeatureCollection, GeoJson};
use libtest_mimic::{Arguments, Failed, Trial};
use medallion::{Query, Root};

const CASES: &str = "../../notebooks/water_crossings/test_cases.geojson";

async fn rail_of(
    root: &Root,
    extract_id: &str,
    ids: &[String],
) -> Result<Vec<Geometry<f64>>, Box<dyn Error>> {
    let segments = root
        .dataset(medallion_model::OVERTURE_EXTRACT)
        .for_id(extract_id)?
        .partition("theme", "transportation")?
        .partition("type", "segment")?;

    let query = Query::new(root.clone());
    query
        .register_at_without_geometry(&segments, "segment")
        .await?;

    let named = ids
        .iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let batches = query
        .sql(&format!(
            "SELECT geometry FROM segment WHERE id IN ({named})"
        ))
        .await?;

    let mut rail = Vec::new();
    for batch in &batches {
        rail.extend(medallion::geometries(batch, "geometry")?);
    }
    Ok(rail)
}

fn slug(name: &str) -> String {
    name.chars()
        .map(|letter| match letter.is_ascii_alphanumeric() {
            true => letter.to_ascii_lowercase(),
            false => '_',
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Arguments::from_args();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = Root::new(Root::default_path()?);

    let cases: FeatureCollection = std::fs::read_to_string(here.join(CASES))?
        .parse::<GeoJson>()?
        .try_into()?;

    let runtime = tokio::runtime::Runtime::new()?;
    let crossings: Vec<PlacedCrossing> = runtime.block_on(medallion::rows_of_every_country(
        &root,
        medallion_model::WATER_CROSSING,
        "water_crossing",
        crossings::silver::PLACED,
    ))?;

    let mut trials = Vec::new();
    for case in cases.features {
        let name = case
            .property("name")
            .and_then(|name| name.as_str())
            .ok_or("a case names itself")?
            .to_string();
        let expected = case
            .property("expected_crossings")
            .and_then(|count| count.as_u64())
            .ok_or_else(|| format!("{name} says how many crossings it expects"))?
            as usize;
        let bbox: Geometry<f64> = case
            .geometry
            .ok_or_else(|| format!("{name} covers a bbox"))?
            .try_into()?;

        let inside: Vec<&PlacedCrossing> = crossings
            .iter()
            .filter(|crossing| bbox.contains(&Point::new(crossing.lon, crossing.lat)))
            .collect();

        let mut by_extract: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for crossing in &inside {
            by_extract
                .entry(&crossing.row.extract_id)
                .or_default()
                .push(crossing.row.rail_id.clone());
        }
        let mut rail = Vec::new();
        for (extract_id, ids) in by_extract {
            rail.extend(runtime.block_on(rail_of(&root, extract_id, &ids))?);
        }

        let found = inside.len();
        let named: Vec<String> = inside
            .iter()
            .map(|crossing| crossing.row.crossing_id.to_string())
            .collect();

        trials.push(Trial::test(
            format!("{}::crossings", slug(&name)),
            move || -> Result<(), Failed> {
                if found != expected {
                    return Err(
                        format!("expected {expected} crossings, found {found}: {named:?}").into(),
                    );
                }
                let adrift = rail.iter().filter(|segment| !segment.intersects(&bbox)).count();
                if adrift > 0 {
                    return Err(format!(
                        "{adrift} of the {} rail segments these crossings sit on do not reach the case",
                        rail.len()
                    )
                    .into());
                }
                Ok(())
            },
        ));
    }

    libtest_mimic::run(&args, trials).exit()
}
