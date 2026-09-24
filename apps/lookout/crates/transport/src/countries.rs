use geo::Contains;
use geo_types::{Geometry, Point};
use medallion::{Countries, Country, GEOMETRY, Query, Root};

const NEWEST_EXTRACT: &str = "
    SELECT extract_id FROM extract_manifest ORDER BY extracted_at DESC LIMIT 1
";

#[derive(Debug, thiserror::Error)]
pub enum CountryError {
    #[error("reading the extracts: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("reading the country areas: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the extract: {0}")]
    Path(#[from] medallion::PathError),
    #[error("no extract has been taken, so there are no country areas to place a point in")]
    NoExtract,
}

#[derive(Debug, Clone, Default)]
pub struct CountryAreas {
    areas: Vec<(Country, Geometry<f64>)>,
}

#[derive(Debug, serde::Deserialize)]
struct Extracted {
    extract_id: String,
}

impl CountryAreas {
    pub async fn newest(root: &Root) -> Result<Self, CountryError> {
        let query = Query::new(root.clone());
        query
            .register_by_name(medallion_model::EXTRACT_MANIFEST)
            .await?;
        let newest: Vec<Extracted> = query.rows(NEWEST_EXTRACT).await?;
        let newest = newest.first().ok_or(CountryError::NoExtract)?;

        let areas = root
            .dataset(medallion_model::OVERTURE_EXTRACT)
            .for_id(&newest.extract_id)?
            .partition("theme", "divisions")?
            .partition("type", "division_area")?;
        query.register_at(&areas, "division_area").await?;

        let mut areas = Vec::new();
        for country in Country::ALL {
            let batches = query
                .sql(&format!(
                    "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY}
                     FROM division_area
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
    use geo_types::{Geometry, polygon};

    use super::*;

    fn square(size: f64) -> Geometry<f64> {
        Geometry::Polygon(polygon![
            (x: 0.0, y: 0.0),
            (x: size, y: 0.0),
            (x: size, y: size),
            (x: 0.0, y: size),
        ])
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

        let err = CountryAreas::newest(&Root::new(tmp.path())).await;

        assert!(matches!(
            err,
            Err(CountryError::Query(
                medallion::QueryError::NoSuchDataset { .. }
            ))
        ));
    }
}
