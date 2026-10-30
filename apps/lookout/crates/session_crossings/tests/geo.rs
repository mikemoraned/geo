use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::error::Error;
use std::path::Path;

use domain::{CrossingId, SessionId};
use geojson::{Feature, FeatureCollection, GeoJson};
use libtest_mimic::{Arguments, Failed, Trial};
use medallion::{Country, Root};
use session_crossings::matching::{DEFAULT_RADIUS, steps};
use session_crossings::silver::{Inputs, inputs};

const CASES: &str = "tests/passes.geojson";

struct Case {
    name: String,
    country: Country,
    session_id: SessionId,
    crossing_id: CrossingId,
    expected_passes: usize,
}

fn property<'a>(feature: &'a Feature, key: &str) -> Result<&'a serde_json::Value, String> {
    feature
        .property(key)
        .ok_or_else(|| format!("a case states its `{key}`"))
}

fn text<'a>(feature: &'a Feature, key: &str) -> Result<&'a str, String> {
    property(feature, key)?
        .as_str()
        .ok_or_else(|| format!("a case's `{key}` is text"))
}

fn case_of(feature: &Feature) -> Result<Case, Box<dyn Error>> {
    Ok(Case {
        name: text(feature, "name")?.to_string(),
        country: text(feature, "country")?.parse()?,
        session_id: text(feature, "session_id")?.parse()?,
        crossing_id: CrossingId::new(text(feature, "crossing_id")?)?,
        expected_passes: property(feature, "expected_passes")?
            .as_u64()
            .ok_or("a case's `expected_passes` is a count")?
            .try_into()?,
    })
}

fn passes_found(case: &Case, inputs: &Inputs) -> Result<usize, String> {
    let session = inputs
        .sessions
        .iter()
        .find(|session| session.session_id == case.session_id)
        .ok_or_else(|| format!("the store holds no session {}", case.session_id))?;
    let crossing = inputs
        .crossings
        .iter()
        .find(|crossing| crossing.crossing.id == case.crossing_id)
        .ok_or_else(|| format!("the store holds no crossing {}", case.crossing_id.as_str()))?;
    Ok(steps(
        &session.session_id,
        &session.samples,
        crossing,
        DEFAULT_RADIUS,
    )
    .map_err(|err| err.to_string())?
    .passes
    .len())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Arguments::from_args();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = Root::new(Root::default_path()?);

    let cases: FeatureCollection = std::fs::read_to_string(here.join(CASES))?
        .parse::<GeoJson>()?
        .try_into()?;
    let cases = cases
        .features
        .iter()
        .map(case_of)
        .collect::<Result<Vec<_>, _>>()?;

    let runtime = tokio::runtime::Runtime::new()?;
    let mut by_country: HashMap<Country, Inputs> = HashMap::new();
    for case in &cases {
        if let Entry::Vacant(vacant) = by_country.entry(case.country) {
            vacant.insert(runtime.block_on(inputs(&root, case.country))?);
        }
    }

    let trials = cases
        .into_iter()
        .map(|case| {
            let found = passes_found(&case, &by_country[&case.country]);
            let expected = case.expected_passes;
            Trial::test(
                format!("{}::passes", case.name),
                move || -> Result<(), Failed> {
                    let found = found?;
                    println!("expected {expected} passes, found {found}");
                    match found == expected {
                        true => Ok(()),
                        false => Err(format!("expected {expected} passes, found {found}").into()),
                    }
                },
            )
        })
        .collect();

    libtest_mimic::run(&args, trials).exit()
}
