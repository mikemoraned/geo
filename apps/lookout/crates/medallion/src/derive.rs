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

#[derive(Debug, Clone, PartialEq)]
pub struct GeoRow<R, G> {
    pub row: R,
    pub geometry: G,
    pub country: Country,
}

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

    let partition_of_each_row: Vec<(Country, NaiveDate)> = rows
        .iter()
        .map(|placed| (placed.country, placed.row.partition_date()))
        .collect();
    let mut projectors: HashMap<Country, Projector> = HashMap::new();
    let mut by_country: Vec<(Country, Vec<(NaiveDate, RecordBatch)>)> = Vec::new();

    for ((country, date), indices) in group(&partition_of_each_row) {
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

    let every_country_the_rows_cover: Vec<Country> =
        by_country.iter().map(|(country, _)| *country).collect();
    partitions.removed += root
        .dataset(target.spec())
        .retain_partitions(COUNTRY, &every_country_the_rows_cover)
        .await?;

    Ok(TableWritten {
        rows: rows.len(),
        partitions,
    })
}

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
