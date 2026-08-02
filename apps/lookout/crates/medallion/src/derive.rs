//! Writing a silver dataset from rows of a Rust type.
//!
//! This is the door a derivation written in Rust goes through, as [`crate::write_table`] is
//! the door for one written elsewhere. Both apply the same policy — the dataset's layout, the
//! sweep of what a rebuild no longer produces, and the uniqueness its definition declares —
//! so which language derived a dataset does not change what is stored.
//!
//! The caller supplies rows and, for a dataset that carries geometry, the lat/lon geometry
//! and the country each row belongs to; everything else follows from the definition:
//!
//! * **The date** a row is stored under is read from the row itself, through [`Dated`], so
//!   the pairing of a partition key with the column feeding it is stated where the dataset is
//!   defined.
//! * **The projected geometry** is derived here rather than supplied, from the row's country:
//!   the zone a country's metres are in is the store's choice, and projecting into one while
//!   declaring another is the mistake this removes the opportunity for.
//! * **Partitions** are replaced, and the ones the rows no longer cover — dates within a
//!   country, and the countries themselves — are deleted.
//!
//! A run therefore has to derive the whole dataset, which is the rule silver rebuilds already
//! follow.
//!
//! **Silver only**, though the layer below permits a gold dataset to be replaced too. What is
//! written here is the silver format — WKB geometry with its metric twin, the CRS declared per
//! country — and what is deleted follows silver's rule that a partition a rebuild no longer
//! produces is a claim withdrawn. Gold states neither: its format is the consumer's, and its
//! outputs are versioned per run rather than replaced, precisely so an earlier one survives.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use arrow::array::RecordBatch;
use chrono::NaiveDate;

use crate::country::{COUNTRY, Country};
use crate::geo::{
    GEOMETRY, PROJECTED_GEOMETRY, Projector, geo_batch, projected_wkb_field, wkb_field,
};
use crate::layer::layers;
use crate::path::{Replaced, Root};
use crate::rows::{Dated, Row, batch};
use crate::table::{
    Layout, SilverTarget, TableError, TableWritten, check_unique, group, replace_dates,
};

/// One row of a dataset that carries geometry: the row, the geometry its columns hold in
/// lat/lon, and the country whose zone the metric column is written in.
///
/// The country is stated rather than looked up from the geometry, because which point places
/// a row is a question about the entity: a session's samples belong to the country the
/// session started in, whatever ground the session later covered, so that they and the
/// session they make up are measured in the same metres.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoRow<R, G> {
    pub row: R,
    pub geometry: G,
    pub country: Country,
}

/// Write `rows` as the whole of the dataset they belong to, replacing what is there.
///
/// Each partition holds the rows given for it in the order they were given, and a partition
/// the rows no longer cover is deleted.
pub async fn write_geo_rows<R, G>(
    root: &Root,
    rows: &[GeoRow<R, G>],
) -> Result<TableWritten, TableError>
where
    R: Dated<Layer = layers::Silver> + Clone,
    G: geo_traits::GeometryTrait<T = f64> + geo::MapCoords<f64, f64, Output = G> + Clone,
{
    let target = SilverTarget::of::<R>()?;
    let Layout::CountryAndDate(_) = target.layout()? else {
        return Err(TableError::GeometryUnexpected {
            dataset: target.name(),
        });
    };
    check_named(&target, rows.iter().map(|placed| &placed.row))?;

    // One batch per country and date, since that pair names a partition. Grouped rather than
    // chunked, so the rows need not arrive in any particular order to land in one file each.
    let keys: Vec<(Country, NaiveDate)> = rows
        .iter()
        .map(|placed| (placed.country, placed.row.partition_date()))
        .collect();
    let mut projectors: HashMap<Country, Projector> = HashMap::new();
    let mut by_country: Vec<(Country, Vec<(NaiveDate, RecordBatch)>)> = Vec::new();

    for ((country, date), indices) in group(&keys) {
        // One projector per country, built once: constructing it is the expensive part, and
        // a country's zone is the same in every partition below it.
        let projector = match projectors.entry(country) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(Projector::for_country(country)?),
        };
        let day: Vec<&GeoRow<R, G>> = indices.iter().map(|row| &rows[*row as usize]).collect();
        let batch = geo_day(&day, projector, country)?;

        match by_country.iter_mut().find(|(each, _)| *each == country) {
            Some((_, days)) => days.push((date, batch)),
            None => by_country.push((country, vec![(date, batch)])),
        }
    }

    let mut partitions = Replaced::default();
    for (country, days) in &by_country {
        let dataset = root.dataset(target.spec()).partition(COUNTRY, *country)?;
        partitions += replace_dates(&dataset, &target, days).await?;
    }

    // The dated partitions of a country are swept as that country is written, which is the
    // level `replace_dates` is given; the countries themselves can only be swept here, where
    // every one the rows cover is known.
    let derived: Vec<Country> = by_country.iter().map(|(country, _)| *country).collect();
    partitions.removed += root
        .dataset(target.spec())
        .retain_partitions(COUNTRY, &derived)
        .await?;

    Ok(TableWritten {
        rows: rows.len(),
        partitions,
    })
}

/// Write `rows` as the whole of the dataset they belong to, for a dataset carrying no
/// geometry — dated partitions and nothing above them. Replaces and sweeps as
/// [`write_geo_rows`] does.
pub async fn write_rows<R>(root: &Root, rows: &[R]) -> Result<TableWritten, TableError>
where
    R: Dated<Layer = layers::Silver> + Clone,
{
    let target = SilverTarget::of::<R>()?;
    let Layout::Date(_) = target.layout()? else {
        return Err(TableError::GeometryMissing {
            dataset: target.name(),
        });
    };
    check_named(&target, rows.iter())?;

    let dates: Vec<NaiveDate> = rows.iter().map(Dated::partition_date).collect();
    let days = group(&dates)
        .into_iter()
        .map(|(date, indices)| {
            let day: Vec<R> = indices
                .iter()
                .map(|row| rows[*row as usize].clone())
                .collect();
            Ok((date, batch(&day)?))
        })
        .collect::<Result<Vec<_>, TableError>>()?;

    let partitions = replace_dates(&root.dataset(target.spec()), &target, &days).await?;
    Ok(TableWritten {
        rows: rows.len(),
        partitions,
    })
}

/// One partition's batch: the rows, then their geometry in lat/lon and in `country`'s metres.
fn geo_day<R, G>(
    day: &[&GeoRow<R, G>],
    projector: &Projector,
    country: Country,
) -> Result<RecordBatch, TableError>
where
    R: Row + Clone,
    G: geo_traits::GeometryTrait<T = f64> + geo::MapCoords<f64, f64, Output = G> + Clone,
{
    let rows: Vec<R> = day.iter().map(|placed| placed.row.clone()).collect();
    let geometry: Vec<G> = day.iter().map(|placed| placed.geometry.clone()).collect();
    let projected: Vec<G> = geometry
        .iter()
        .map(|geometry| projector.project(geometry))
        .collect::<Result<_, _>>()?;

    Ok(geo_batch(
        &rows,
        &[
            (wkb_field(GEOMETRY)?, geometry.as_slice()),
            (
                projected_wkb_field(PROJECTED_GEOMETRY, country)?,
                projected.as_slice(),
            ),
        ],
    )?)
}

/// Refuse rows two of which share a name the dataset declares unique.
///
/// The check reads the columns as the store holds them, which means building them, so it is
/// only paid for by a dataset that declares a name.
fn check_named<'a, R: Row + Clone + 'a>(
    target: &SilverTarget,
    rows: impl Iterator<Item = &'a R>,
) -> Result<(), TableError> {
    if R::UNIQUE.is_empty() {
        return Ok(());
    }
    let rows: Vec<R> = rows.cloned().collect();
    check_unique(target, &batch(&rows)?)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use geo_types::{LineString, Point};
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::dataset::DatasetSpec;
    use crate::query::Query;
    use crate::rows::Geometry;

    /// Dated geometry: a country partition above the date, since the file states one CRS.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TrackRow {
        track_id: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        seen_at: DateTime<Utc>,
    }

    impl Row for TrackRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("track", "seen_date");
        const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
        const INSTANTS: &'static [&'static str] = &["seen_at"];
        const UNIQUE: &'static [&'static str] = &["track_id"];
    }

    impl Dated for TrackRow {
        fn partition_date(&self) -> NaiveDate {
            self.seen_at.date_naive()
        }
    }

    /// Dated rows carrying no geometry: one partition per date, nothing above it.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct PassRow {
        track_id: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        crossed_at: DateTime<Utc>,
    }

    impl Row for PassRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("pass", "crossed_date");
        const INSTANTS: &'static [&'static str] = &["crossed_at"];
        const UNIQUE: &'static [&'static str] = &["track_id"];
    }

    impl Dated for PassRow {
        fn partition_date(&self) -> NaiveDate {
            self.crossed_at.date_naive()
        }
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, day, 9, 0, 0).unwrap()
    }

    fn berlin() -> Point<f64> {
        Point::new(13.404954, 52.520008)
    }

    /// A line through one point, so a track has a geometry without the coordinates being the
    /// subject of the test.
    fn track(id: &str, day: u32, country: Country) -> GeoRow<TrackRow, LineString<f64>> {
        GeoRow {
            row: TrackRow {
                track_id: id.to_string(),
                seen_at: at(day),
            },
            geometry: LineString::from(vec![berlin(), berlin()]),
            country,
        }
    }

    fn pass(id: &str, day: u32) -> PassRow {
        PassRow {
            track_id: id.to_string(),
            crossed_at: at(day),
        }
    }

    #[tokio::test]
    async fn rows_land_in_one_file_per_country_and_date() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());

        let written = write_geo_rows(
            &root,
            &[
                track("a", 21, Country::Germany),
                track("b", 22, Country::Germany),
            ],
        )
        .await
        .unwrap();

        assert_eq!(written.rows, 2);
        assert_eq!(written.partitions.written, 2);
        assert!(
            tmp.path()
                .join("silver/track/country=DE/seen_date=2026-07-21/part-0.parquet")
                .exists()
        );
    }

    /// A partition is decided by what a row holds, not by where it sits among the others, so
    /// rows of one partition arriving apart still land in one file rather than in two writes
    /// of which the second wins.
    #[tokio::test]
    async fn rows_of_one_partition_need_not_arrive_together() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());

        let written = write_geo_rows(
            &root,
            &[
                track("a", 21, Country::Germany),
                track("b", 22, Country::Germany),
                track("c", 21, Country::Germany),
            ],
        )
        .await
        .unwrap();

        assert_eq!(written.partitions.written, 2);
        let query = Query::new(root);
        query.register(TrackRow::DATASET, "track").await.unwrap();
        assert_eq!(
            query
                .count("SELECT COUNT(*) AS count FROM track WHERE seen_date = '2026-07-21'")
                .await
                .unwrap(),
            2
        );
    }

    /// The metric column is projected here rather than supplied, so it is in the zone the
    /// file declares for the country the row states.
    #[tokio::test]
    async fn the_projected_column_holds_the_countrys_metres() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write_geo_rows(&root, &[track("a", 21, Country::Germany)])
            .await
            .unwrap();

        let query = Query::new(root);
        query.register(TrackRow::DATASET, "track").await.unwrap();
        let batches = query
            .sql(&format!(
                "SELECT ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY} FROM track"
            ))
            .await
            .unwrap();

        let projected = crate::geo::geometries(&batches[0], PROJECTED_GEOMETRY).unwrap();
        let geo_types::Geometry::LineString(line) = &projected[0] else {
            panic!("expected a line, got {:?}", projected[0]);
        };
        let first = line.coords().next().unwrap();
        assert!(
            (first.x - 798_809.63).abs() < 0.01 && (first.y - 5_828_000.60).abs() < 0.01,
            "expected metres in the German zone, got {first:?}"
        );
    }

    /// The check a dataset's definition asks for is applied to a Rust writer as much as to a
    /// table handed in from elsewhere: a name identifies a row across the dataset, so two
    /// rows sharing one are refused even when they would land in different partitions.
    #[tokio::test]
    async fn rows_sharing_a_name_are_refused_across_partitions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());

        let err = write_geo_rows(
            &root,
            &[
                track("a", 21, Country::Germany),
                track("a", 22, Country::Germany),
            ],
        )
        .await
        .unwrap_err();

        assert!(
            matches!(&err, TableError::Duplicate { column, .. } if column == "track_id"),
            "{err}"
        );
        assert!(!tmp.path().join("silver").exists(), "nothing was written");
    }

    #[tokio::test]
    async fn rows_sharing_a_name_are_refused_without_geometry_too() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());

        let err = write_rows(&root, &[pass("a", 21), pass("a", 22)])
            .await
            .unwrap_err();

        assert!(matches!(err, TableError::Duplicate { .. }), "{err}");
    }

    #[tokio::test]
    async fn a_date_the_rows_no_longer_cover_is_swept() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write_rows(&root, &[pass("a", 21), pass("b", 22)])
            .await
            .unwrap();

        let written = write_rows(&root, &[pass("a", 21)]).await.unwrap();

        assert_eq!(written.partitions.removed, 1);
        assert!(
            tmp.path()
                .join("silver/pass/crossed_date=2026-07-21")
                .exists()
        );
        assert!(
            !tmp.path()
                .join("silver/pass/crossed_date=2026-07-22")
                .exists()
        );
    }

    /// A dataset carrying geometry cannot be written without it: doing so would put its rows
    /// under a layout that states no CRS and no country.
    #[tokio::test]
    async fn a_dataset_carrying_geometry_refuses_rows_alone() {
        let tmp = tempfile::tempdir().unwrap();

        let err = write_rows(
            &Root::new(tmp.path()),
            &[TrackRow {
                track_id: "a".to_string(),
                seen_at: at(21),
            }],
        )
        .await
        .unwrap_err();

        assert!(matches!(err, TableError::GeometryMissing { .. }), "{err}");
    }

    #[tokio::test]
    async fn a_dataset_carrying_no_geometry_refuses_rows_with_it() {
        let tmp = tempfile::tempdir().unwrap();

        let err = write_geo_rows(
            &Root::new(tmp.path()),
            &[GeoRow {
                row: pass("a", 21),
                geometry: berlin(),
                country: Country::Germany,
            }],
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, TableError::GeometryUnexpected { .. }),
            "{err}"
        );
    }

    /// Writing nothing is a derivation that produced nothing, which sweeps the dataset away
    /// rather than leaving the last run's partitions standing.
    #[tokio::test]
    async fn no_rows_sweep_what_is_there() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write_geo_rows(&root, &[track("a", 21, Country::Germany)])
            .await
            .unwrap();

        let written = write_geo_rows::<TrackRow, LineString<f64>>(&root, &[])
            .await
            .unwrap();

        assert_eq!(written.rows, 0);
        assert_eq!(written.partitions.removed, 1);
        assert!(!tmp.path().join("silver/track/country=DE").exists());
    }
}
