use geo::Contains;
use geo_types::{Geometry, Point};
use medallion::{Countries, Country, GEOMETRY, Query, Root};

const EXTRACTS: &str = "
    SELECT country, extract_id FROM extract_manifest ORDER BY extracted_at DESC
";

#[derive(Debug, thiserror::Error)]
pub enum CountryError {
    #[error("reading the extracts: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("reading the country areas: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the extract: {0}")]
    Path(#[from] medallion::PathError),
}

#[derive(Debug, Clone, Default)]
pub struct CountryAreas {
    areas: Vec<(Country, Geometry<f64>)>,
}

#[derive(Debug, serde::Deserialize)]
struct Extracted {
    country: String,
    extract_id: String,
}

impl CountryAreas {
    pub async fn newest_per_country(root: &Root) -> Result<Self, CountryError> {
        let query = Query::new(root.clone());
        query
            .register_by_name(medallion_model::EXTRACT_MANIFEST)
            .await?;
        let extracts: Vec<Extracted> = query.rows(EXTRACTS).await?;

        let newest_of_each_country = Country::ALL.into_iter().filter_map(|country| {
            extracts
                .iter()
                .find(|extract| extract.country == country.code())
                .map(|extract| (country, extract.extract_id.as_str()))
        });

        let mut areas = Vec::new();
        for (country, extract_id) in newest_of_each_country {
            let division_area = root
                .dataset(medallion_model::OVERTURE_EXTRACT)
                .for_id(extract_id)?
                .partition("theme", "divisions")?
                .partition("type", "division_area")?;
            let table = format!("division_area_{}", country.code().to_lowercase());
            query.register_at(&division_area, &table).await?;

            let batches = query
                .sql(&format!(
                    "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY}
                     FROM {table}
                     WHERE subtype = 'country' AND country = '{}'",
                    country.code()
                ))
                .await?;
            for batch in &batches {
                areas.extend(
                    medallion::geometries(batch, GEOMETRY)?
                        .into_iter()
                        .map(|area| (country, area)),
                );
            }
        }

        Ok(Self { areas })
    }
}

impl Countries for CountryAreas {
    fn containing(&self, point: Point<f64>) -> Option<Country> {
        self.areas
            .iter()
            .find(|(_, area)| area.contains(&point))
            .map(|(country, _)| *country)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{RecordBatch, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use chrono::{DateTime, TimeZone, Utc};
    use datafusion::execution::SendableRecordBatchStream;
    use geo_types::{Geometry, polygon};
    use medallion_model::ExtractManifestRow;

    use super::*;

    fn square(size: f64) -> Geometry<f64> {
        square_from(0.0, size)
    }

    fn square_from(corner: f64, size: f64) -> Geometry<f64> {
        Geometry::Polygon(polygon![
            (x: corner, y: corner),
            (x: corner + size, y: corner),
            (x: corner + size, y: corner + size),
            (x: corner, y: corner + size),
        ])
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 27, hour, 0, 0).unwrap()
    }

    async fn stream_of(batch: RecordBatch) -> SendableRecordBatchStream {
        datafusion::prelude::SessionContext::new()
            .read_batch(batch)
            .expect("read the batch")
            .execute_stream()
            .await
            .expect("stream the batch")
    }

    fn division_area(country: &str, area: &Geometry<f64>) -> RecordBatch {
        let (geometry_field, geometry) = medallion::wkb_column(
            medallion::wkb_field(GEOMETRY).expect("a geometry field"),
            std::slice::from_ref(area),
        )
        .expect("a geometry column");
        let schema = Schema::new(vec![
            Arc::new(Field::new("subtype", DataType::Utf8, false)),
            Arc::new(Field::new("country", DataType::Utf8, false)),
            geometry_field,
        ]);

        RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(StringArray::from(vec!["country"])),
                Arc::new(StringArray::from(vec![country])),
                geometry,
            ],
        )
        .expect("a division_area batch")
    }

    async fn extracted(
        root: &Root,
        id: &str,
        country: &str,
        release: &str,
        taken_at: DateTime<Utc>,
        area: &Geometry<f64>,
    ) {
        let row = ExtractManifestRow {
            extract_id: id.to_string(),
            extracted_at: taken_at,
            release: release.to_string(),
            country: country.to_string(),
            min_lon: 0.0,
            min_lat: 0.0,
            max_lon: 0.0,
            max_lat: 0.0,
        };
        root.rows_of::<ExtractManifestRow>()
            .append_rows(taken_at, &[row])
            .await
            .expect("record the extract");

        root.dataset(medallion_model::OVERTURE_EXTRACT)
            .for_id(id)
            .expect("an extract id")
            .partition("theme", "divisions")
            .expect("the divisions theme")
            .partition("type", "division_area")
            .expect("the division_area type")
            .append_geo_stream(taken_at, stream_of(division_area(country, area)).await)
            .await
            .expect("write the areas");
    }

    #[test]
    fn a_point_inside_an_area_is_in_that_country() {
        let areas = CountryAreas {
            areas: vec![(Country::Germany, square(10.0))],
        };

        assert_eq!(
            areas.containing(Point::new(5.0, 5.0)),
            Some(Country::Germany)
        );
    }

    #[test]
    fn a_point_outside_every_area_is_in_no_country() {
        let areas = CountryAreas {
            areas: vec![(Country::Germany, square(10.0))],
        };

        assert_eq!(areas.containing(Point::new(20.0, 20.0)), None);
    }

    #[tokio::test]
    async fn a_store_with_no_extract_cannot_place_a_point() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let err = CountryAreas::newest_per_country(&Root::new(tmp.path())).await;

        assert!(matches!(
            err,
            Err(CountryError::Query(
                medallion::QueryError::NoSuchDataset { .. }
            ))
        ));
    }
    #[tokio::test]
    async fn a_later_extract_of_one_country_leaves_the_others_points_placeable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        extracted(
            &root,
            "20260927T090000Z",
            Country::Germany.code(),
            "2026-07-22.0",
            at(9),
            &square(10.0),
        )
        .await;
        extracted(
            &root,
            "20260927T190000Z",
            "GB",
            "2026-07-22.0",
            at(19),
            &square_from(20.0, 10.0),
        )
        .await;

        let areas = CountryAreas::newest_per_country(&root)
            .await
            .expect("the country areas");

        assert_eq!(
            areas.containing(Point::new(5.0, 5.0)),
            Some(Country::Germany)
        );
    }

    #[tokio::test]
    async fn a_country_extracted_from_an_older_release_places_its_points_the_same() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        extracted(
            &root,
            "20260927T090000Z",
            "GB",
            "2026-07-22.0",
            at(9),
            &square_from(20.0, 10.0),
        )
        .await;
        extracted(
            &root,
            "20260927T190000Z",
            Country::Germany.code(),
            "2026-05-21.0",
            at(19),
            &square(10.0),
        )
        .await;

        let areas = CountryAreas::newest_per_country(&root)
            .await
            .expect("the country areas");

        assert_eq!(
            areas.containing(Point::new(5.0, 5.0)),
            Some(Country::Germany)
        );
    }

    #[tokio::test]
    async fn a_second_extract_of_a_country_supersedes_the_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        extracted(
            &root,
            "20260927T090000Z",
            Country::Germany.code(),
            "2026-05-21.0",
            at(9),
            &square(10.0),
        )
        .await;
        extracted(
            &root,
            "20260927T190000Z",
            Country::Germany.code(),
            "2026-07-22.0",
            at(19),
            &square_from(20.0, 10.0),
        )
        .await;

        let areas = CountryAreas::newest_per_country(&root)
            .await
            .expect("the country areas");

        assert_eq!(
            areas.containing(Point::new(25.0, 25.0)),
            Some(Country::Germany)
        );
        assert_eq!(areas.containing(Point::new(5.0, 5.0)), None);
    }
}
