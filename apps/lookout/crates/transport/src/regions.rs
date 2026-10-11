use std::fmt::{self, Display};
use std::str::FromStr;

use geo_types::Geometry;
use medallion::{Country, GEOMETRY, Query, Root};
use medallion_model::{DIVISION_ID, ExtractManifestRow, GersId, NotAGersId};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionCode(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{code}` is not an Overture region code, such as DE-TH")]
pub struct NotARegionCode {
    code: String,
}

impl RegionCode {
    pub fn new(code: impl Into<String>) -> Result<Self, NotARegionCode> {
        let code = code.into();
        let well_formed = match code.split_once('-') {
            Some((country, subdivision)) => {
                country.len() == 2
                    && country.chars().all(|c| c.is_ascii_uppercase())
                    && (1..=3).contains(&subdivision.len())
                    && subdivision
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            }
            None => false,
        };
        if well_formed {
            Ok(Self(code))
        } else {
            Err(NotARegionCode { code })
        }
    }

    pub fn country_code(&self) -> &str {
        &self.0[..2]
    }
}

impl FromStr for RegionCode {
    type Err = NotARegionCode;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Self::new(code)
    }
}

impl Display for RegionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionArea {
    pub id: GersId,
    pub name: String,
    pub shapes: Vec<Geometry<f64>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    #[error("reading the extract: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("reading the region's shape: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the extract: {0}")]
    Path(#[from] medallion::PathError),
    #[error("the store holds no extract of {country}")]
    NoExtract { country: Country },
    #[error("region {code} is not in {country}")]
    OtherCountry { code: RegionCode, country: Country },
    #[error("the newest extract of {country} holds no region {code}")]
    NoSuchRegion { code: RegionCode, country: Country },
    #[error("the newest extract of {country} names region {code} with several divisions")]
    SeveralDivisions { code: RegionCode, country: Country },
    #[error(transparent)]
    Division(#[from] NotAGersId),
}

#[derive(Debug, Deserialize)]
struct Division {
    division_id: String,
    name: String,
}

pub async fn region_area(
    root: &Root,
    country: Country,
    code: &RegionCode,
) -> Result<RegionArea, RegionError> {
    if code.country_code() != country.code() {
        return Err(RegionError::OtherCountry {
            code: code.clone(),
            country,
        });
    }
    let query = Query::new(root.clone());
    query
        .register_by_name(medallion_model::EXTRACT_MANIFEST)
        .await?;
    let extracts: Vec<ExtractManifestRow> = query.rows(crate::extract::RECORDED).await?;
    let extract_id = extracts
        .iter()
        .find(|extract| extract.country == country.code())
        .map(|extract| extract.extract_id.clone())
        .ok_or(RegionError::NoExtract { country })?;

    let division_area = root
        .dataset(medallion_model::OVERTURE_EXTRACT)
        .for_id(&extract_id)?
        .partition("theme", "divisions")?
        .partition("type", "division_area")?;
    query
        .register_at_without_geometry(&division_area, "division_area")
        .await?;
    let of_the_region = format!("WHERE subtype = 'region' AND region = '{code}'");

    let divisions: Vec<Division> = query
        .rows(&format!(
            "SELECT DISTINCT {DIVISION_ID} AS division_id, names['primary'] AS name
             FROM division_area {of_the_region}"
        ))
        .await?;
    let division = match divisions.as_slice() {
        [division] => division,
        [] => {
            return Err(RegionError::NoSuchRegion {
                code: code.clone(),
                country,
            });
        }
        _ => {
            return Err(RegionError::SeveralDivisions {
                code: code.clone(),
                country,
            });
        }
    };

    let mut shapes = Vec::new();
    for batch in query
        .sql(&format!(
            "SELECT {GEOMETRY} FROM division_area {of_the_region}"
        ))
        .await?
    {
        shapes.extend(medallion::geometries(&batch, GEOMETRY)?);
    }

    Ok(RegionArea {
        id: division.division_id.parse()?,
        name: division.name.clone(),
        shapes,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{ArrayRef, RecordBatch, StringArray, StructArray};
    use arrow::datatypes::{DataType, Field, Fields, Schema};
    use chrono::{DateTime, TimeZone, Utc};
    use datafusion::execution::SendableRecordBatchStream;
    use geo_types::polygon;

    use super::*;

    const THURINGIA: &str = "e0ff5492-5758-4335-aabd-af1747925cd1";
    const SAXONY: &str = "3e1aa55a-1b9b-4d9b-8072-38ba413f6d2b";

    fn square_from(corner: f64, size: f64) -> Geometry<f64> {
        Geometry::Polygon(polygon![
            (x: corner, y: corner),
            (x: corner + size, y: corner),
            (x: corner + size, y: corner + size),
            (x: corner, y: corner + size),
        ])
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 8, 9, 0, 0).unwrap()
    }

    struct Area<'a> {
        division: &'a str,
        region: &'a str,
        name: &'a str,
        shape: Geometry<f64>,
    }

    fn division_area(areas: &[Area<'_>]) -> RecordBatch {
        let geometries: Vec<Geometry<f64>> = areas.iter().map(|area| area.shape.clone()).collect();
        let (geometry_field, geometry) = medallion::wkb_column(
            medallion::wkb_field(GEOMETRY).expect("a geometry field"),
            &geometries,
        )
        .expect("a geometry column");
        let primary: ArrayRef = Arc::new(StringArray::from(
            areas.iter().map(|area| area.name).collect::<Vec<_>>(),
        ));
        let names_fields = Fields::from(vec![Field::new("primary", DataType::Utf8, false)]);
        let names = StructArray::new(names_fields.clone(), vec![primary], None);
        let schema = Schema::new(vec![
            Arc::new(Field::new(DIVISION_ID, DataType::Utf8, false)),
            Arc::new(Field::new("subtype", DataType::Utf8, false)),
            Arc::new(Field::new("region", DataType::Utf8, false)),
            Arc::new(Field::new("names", DataType::Struct(names_fields), false)),
            geometry_field,
        ]);

        RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(StringArray::from(
                    areas.iter().map(|area| area.division).collect::<Vec<_>>(),
                )),
                Arc::new(StringArray::from(vec!["region"; areas.len()])),
                Arc::new(StringArray::from(
                    areas.iter().map(|area| area.region).collect::<Vec<_>>(),
                )),
                Arc::new(names),
                geometry,
            ],
        )
        .expect("a division_area batch")
    }

    async fn stream_of(batch: RecordBatch) -> SendableRecordBatchStream {
        datafusion::prelude::SessionContext::new()
            .read_batch(batch)
            .expect("read the batch")
            .execute_stream()
            .await
            .expect("stream the batch")
    }

    async fn extracted(root: &Root, areas: &[Area<'_>]) {
        let id = "20261008T090000Z";
        root.rows_of::<ExtractManifestRow>()
            .append_rows(
                at(),
                &[ExtractManifestRow {
                    extract_id: id.to_string(),
                    extracted_at: at(),
                    release: "2026-09-17.0".to_string(),
                    country: Country::Germany.code().to_string(),
                    min_lon: 0.0,
                    min_lat: 0.0,
                    max_lon: 0.0,
                    max_lat: 0.0,
                }],
            )
            .await
            .expect("record the extract");
        root.dataset(medallion_model::OVERTURE_EXTRACT)
            .for_id(id)
            .expect("an extract id")
            .partition("theme", "divisions")
            .expect("the divisions theme")
            .partition("type", "division_area")
            .expect("the division_area type")
            .append_geo_stream(at(), stream_of(division_area(areas)).await)
            .await
            .expect("write the areas");
    }

    fn thuringia_and_saxony() -> Vec<Area<'static>> {
        vec![
            Area {
                division: THURINGIA,
                region: "DE-TH",
                name: "Thüringen",
                shape: square_from(0.0, 10.0),
            },
            Area {
                division: SAXONY,
                region: "DE-SN",
                name: "Sachsen",
                shape: square_from(20.0, 10.0),
            },
        ]
    }

    fn code(text: &str) -> RegionCode {
        text.parse().expect("a region code")
    }

    #[test]
    fn a_region_code_is_a_country_code_a_hyphen_and_a_subdivision() {
        for good in ["DE-TH", "GB-ENG", "FR-75"] {
            assert!(RegionCode::new(good).is_ok(), "{good}");
        }
        for bad in ["TH", "de-th", "DE-", "-TH", "DE-THUR", "DEU-TH", ""] {
            assert!(RegionCode::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_region_code_names_its_country() {
        assert_eq!(code("DE-TH").country_code(), "DE");
    }

    #[tokio::test]
    async fn a_region_is_found_by_its_code_with_its_id_name_and_shape() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        extracted(&root, &thuringia_and_saxony()).await;

        let area = region_area(&root, Country::Germany, &code("DE-TH"))
            .await
            .expect("Thuringia");

        assert_eq!(area.id.to_string(), THURINGIA);
        assert_eq!(area.name, "Thüringen");
        assert_eq!(area.shapes, vec![square_from(0.0, 10.0)]);
    }

    #[tokio::test]
    async fn a_region_the_extract_lacks_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        extracted(&root, &thuringia_and_saxony()).await;

        let err = region_area(&root, Country::Germany, &code("DE-BY")).await;

        assert!(
            matches!(err, Err(RegionError::NoSuchRegion { .. })),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_region_of_another_country_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let err = region_area(&Root::new(tmp.path()), Country::Germany, &code("GB-ENG")).await;

        assert!(
            matches!(err, Err(RegionError::OtherCountry { .. })),
            "{err:?}"
        );
    }
}
