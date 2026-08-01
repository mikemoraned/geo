//! Which country a place is in, from the country areas an extract took.
//!
//! The areas are the reference data's own: the `division_area` rows of subtype `country`,
//! read out of bronze rather than approximated by a bounding box, so "which country is this
//! point in" is answered by the same boundaries everything else here is derived against.
//!
//! Only the countries the store knows a projected zone for are loaded — an area of any
//! other is not something a derivation could write geometry for, so it would only be able
//! to answer with a country nothing can be done with.

use geo::Contains;
use geo_types::{Geometry, Point};
use medallion::{Countries, Country, GEOMETRY, Query, Root};

/// The newest extract's country areas.
const NEWEST_EXTRACT: &str = "
    SELECT extract_id FROM manifest ORDER BY extracted_at DESC LIMIT 1
";

/// A failure loading the country areas.
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

/// The area of each country the store knows a projected zone for.
#[derive(Debug, Clone, Default)]
pub struct CountryAreas {
    areas: Vec<(Country, Geometry<f64>)>,
}

/// One country area as the extract holds it.
#[derive(Debug, serde::Deserialize)]
struct Extracted {
    extract_id: String,
}

impl CountryAreas {
    /// Load the areas from the newest extract in `root`.
    pub async fn newest(root: &Root) -> Result<Self, CountryError> {
        let query = Query::new(root.clone());
        query.register(model::EXTRACT_MANIFEST, "manifest").await?;
        let newest: Vec<Extracted> = query.rows(NEWEST_EXTRACT).await?;
        let newest = newest.first().ok_or(CountryError::NoExtract)?;

        let areas = root
            .dataset(model::OVERTURE_EXTRACT)
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

    /// A square of `size` degrees with its south-west corner at the origin.
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

    /// A place outside every known country is not attributed to one: there is no zone to
    /// project it into, and guessing the nearest would put geometry in the wrong metres.
    #[test]
    fn a_point_outside_every_area_is_in_no_country() {
        let areas = CountryAreas {
            areas: vec![(Country::Germany, square(10.0))],
        };

        assert_eq!(areas.containing(Point::new(20.0, 20.0)), None);
    }

    /// A store nothing has been extracted into cannot place a point, and says so rather
    /// than answering as though everywhere were unknown territory.
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
