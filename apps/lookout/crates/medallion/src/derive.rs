use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::marker::PhantomData;

use arrow::array::RecordBatch;
use chrono::NaiveDate;

use crate::country::{COUNTRY, Country};
use crate::geo::{
    GeoParquetFile, Projectors, ZonedGeometry, batch_with_geometry_first, geometry_fields,
    wkb_column,
};
use crate::layer::layers;
use crate::partition::DATE_FORMAT;
use crate::path::{Replaced, Root};
use crate::rows::{Dated, Row, batch};
use crate::table::{
    Layout, SilverTarget, TableError, TableWritten, check_unique, group, replace_dates,
};

#[derive(Debug, Clone, PartialEq)]
pub struct GeographyRow<R, G> {
    pub row: R,
    pub geography: G,
    pub country: Country,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeometryRow<R, G> {
    pub row: R,
    pub geometry: ZonedGeometry<G>,
}

pub async fn write_geography_rows<R, G>(
    root: &Root,
    rows: &[GeographyRow<R, G>],
) -> Result<TableWritten, TableError>
where
    R: Dated<Layer = layers::Silver> + Clone,
    G: geo_traits::GeometryTrait<T = f64> + geo::MapCoords<f64, f64, Output = G>,
{
    let mut writer = GeometryRowWriter::<R>::for_any_rows(root)?;
    check_named(&writer.target, rows.iter().map(|placed| &placed.row))?;
    let mut projectors = Projectors::default();
    let days = days_of(rows, |placed| (placed.country, placed.row.partition_date()));
    for (partition, day) in days {
        let projector = projectors.for_country(partition.0)?;
        let projected: Vec<G> = day
            .iter()
            .map(|placed| projector.project(&placed.geography))
            .collect::<Result<_, _>>()?;
        let rows: Vec<&R> = day.iter().map(|placed| &placed.row).collect();
        let batch = geo_day(
            &rows,
            day.iter().map(|placed| &placed.geography),
            &projected,
            partition.0,
        )?;
        writer.write_day(partition, batch).await?;
        writer.close_day(partition).await?;
    }
    writer.finish().await
}

type Partition = (Country, NaiveDate);

pub struct GeometryRowWriter<R> {
    root: Root,
    target: SilverTarget,
    date_key: &'static str,
    open: HashMap<Partition, GeoParquetFile>,
    closed: Vec<Partition>,
    rows: usize,
    row: PhantomData<R>,
}

impl<R> GeometryRowWriter<R>
where
    R: Dated<Layer = layers::Silver>,
{
    pub fn new(root: &Root) -> Result<Self, TableError> {
        let writer = Self::for_any_rows(root)?;
        if !R::UNIQUE.is_empty() {
            return Err(TableError::UniqueAcrossWrites {
                dataset: writer.target.name(),
            });
        }
        Ok(writer)
    }

    fn for_any_rows(root: &Root) -> Result<Self, TableError> {
        let target = SilverTarget::of::<R>()?;
        let Layout::CountryAndDate(date_key) = target.layout()? else {
            return Err(TableError::GeometryUnexpected {
                dataset: target.name(),
            });
        };
        Ok(Self {
            root: root.clone(),
            target,
            date_key,
            open: HashMap::new(),
            closed: Vec::new(),
            rows: 0,
            row: PhantomData,
        })
    }

    pub async fn write<G>(&mut self, rows: &[GeometryRow<R, G>]) -> Result<(), TableError>
    where
        G: geo_traits::GeometryTrait<T = f64>,
    {
        let days = days_of(rows, |zoned| {
            (zoned.geometry.country(), zoned.row.partition_date())
        });
        for (partition, day) in days {
            let rows: Vec<&R> = day.iter().map(|zoned| &zoned.row).collect();
            let batch = geo_day(
                &rows,
                day.iter().map(|zoned| zoned.geometry.geography()),
                day.iter().map(|zoned| zoned.geometry.projected()),
                partition.0,
            )?;
            self.write_day(partition, batch).await?;
        }
        Ok(())
    }

    async fn write_day(
        &mut self,
        (country, date): Partition,
        batch: RecordBatch,
    ) -> Result<(), TableError> {
        if self.closed.contains(&(country, date)) {
            return Err(TableError::DayClosed {
                dataset: self.target.name(),
                country,
                date,
            });
        }
        let file = match self.open.entry((country, date)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let partition = self
                    .root
                    .dataset(self.target.spec())
                    .partition(COUNTRY, country)?
                    .on_date(date)?;
                entry.insert(partition.create_geo_partition(batch.schema().as_ref())?)
            }
        };
        file.write(&batch).await?;

        self.rows += batch.num_rows();
        Ok(())
    }

    pub async fn close_days_before(&mut self, date: NaiveDate) -> Result<(), TableError> {
        let finished: Vec<Partition> = self
            .open
            .keys()
            .filter(|(_, day)| *day < date)
            .copied()
            .collect();
        for partition in finished {
            self.close_day(partition).await?;
        }
        Ok(())
    }

    async fn close_day(&mut self, partition: Partition) -> Result<(), TableError> {
        if let Some(file) = self.open.remove(&partition) {
            file.close().await?;
            self.closed.push(partition);
        }
        Ok(())
    }

    pub async fn finish(self) -> Result<TableWritten, TableError> {
        let written: Vec<Partition> = self
            .closed
            .iter()
            .chain(self.open.keys())
            .copied()
            .collect();
        for file in self.open.into_values() {
            file.close().await?;
        }

        let mut dates_by_country: HashMap<Country, Vec<String>> = HashMap::new();
        for (country, date) in written {
            dates_by_country
                .entry(country)
                .or_default()
                .push(date.format(DATE_FORMAT).to_string());
        }

        let dataset = self.root.dataset(self.target.spec());
        let mut partitions = Replaced {
            written: dates_by_country.values().map(Vec::len).sum(),
            removed: 0,
        };
        for (country, dates) in &dates_by_country {
            partitions.removed += dataset
                .clone()
                .partition(COUNTRY, country)?
                .retain_partitions(self.date_key, dates)
                .await?;
        }
        let countries: Vec<Country> = dates_by_country.into_keys().collect();
        partitions.removed += dataset.retain_partitions(COUNTRY, &countries).await?;

        Ok(TableWritten {
            rows: self.rows,
            partitions,
        })
    }
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

fn days_of<T>(rows: &[T], partition_of: impl Fn(&T) -> Partition) -> Vec<(Partition, Vec<&T>)> {
    let partition_of_each_row: Vec<Partition> = rows.iter().map(partition_of).collect();

    group(&partition_of_each_row)
        .into_iter()
        .map(|(partition, indices)| {
            let day = indices.iter().map(|row| &rows[*row as usize]).collect();
            (partition, day)
        })
        .collect()
}

fn geo_day<'a, R, G>(
    rows: &[&R],
    geography: impl IntoIterator<Item = &'a G>,
    projected: impl IntoIterator<Item = &'a G>,
    country: Country,
) -> Result<RecordBatch, TableError>
where
    R: Row,
    G: geo_traits::GeometryTrait<T = f64> + 'a,
{
    let [geography_field, projected_field] = geometry_fields(country)?;
    Ok(batch_with_geometry_first::<R>(
        rows,
        vec![
            wkb_column(geography_field, geography)?,
            wkb_column(projected_field, projected)?,
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
    use crate::geo::{PROJECTED_GEOMETRY, Projector};
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

        fn example() -> Self {
            TrackRow {
                track_id: "example".to_string(),
                seen_at: DateTime::UNIX_EPOCH,
            }
        }
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

        fn example() -> Self {
            PassRow {
                track_id: "example".to_string(),
                crossed_at: DateTime::UNIX_EPOCH,
            }
        }
    }

    impl Dated for PassRow {
        fn partition_date(&self) -> NaiveDate {
            self.crossed_at.date_naive()
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct SightingRow {
        track_id: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        seen_at: DateTime<Utc>,
    }

    impl Row for SightingRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("sighting", "seen_date");
        const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
        const INSTANTS: &'static [&'static str] = &["seen_at"];

        fn example() -> Self {
            SightingRow {
                track_id: "example".to_string(),
                seen_at: DateTime::UNIX_EPOCH,
            }
        }
    }

    impl Dated for SightingRow {
        fn partition_date(&self) -> NaiveDate {
            self.seen_at.date_naive()
        }
    }

    fn sighting(id: &str, day: u32, country: Country) -> GeometryRow<SightingRow, Point<f64>> {
        GeometryRow {
            row: SightingRow {
                track_id: id.to_string(),
                seen_at: at(day),
            },
            geometry: Projector::for_country(country)
                .unwrap()
                .zoned(berlin())
                .unwrap(),
        }
    }

    async fn write_in_calls(
        root: &Root,
        calls: &[&[GeometryRow<SightingRow, Point<f64>>]],
    ) -> Result<TableWritten, TableError> {
        let mut writer = GeometryRowWriter::<SightingRow>::new(root)?;
        for rows in calls {
            writer.write(rows).await?;
        }
        writer.finish().await
    }

    fn sighting_partition(tmp: &tempfile::TempDir, country: &str, day: u32) -> bool {
        tmp.path()
            .join(format!(
                "silver/sighting/country={country}/seen_date=2026-07-{day}"
            ))
            .exists()
    }

    #[tokio::test]
    async fn rows_written_over_several_calls_are_all_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());

        let written = write_in_calls(
            &root,
            &[
                &[sighting("a", 21, Country::Germany)],
                &[
                    sighting("b", 22, Country::Germany),
                    sighting("c", 22, Country::Germany),
                ],
            ],
        )
        .await
        .unwrap();

        assert_eq!(written.rows, 3);
        assert_eq!(written.partitions.written, 2);
        assert!(sighting_partition(&tmp, "DE", 21) && sighting_partition(&tmp, "DE", 22));
    }

    #[tokio::test]
    async fn a_date_no_call_wrote_is_removed_on_finishing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write_in_calls(&root, &[&[sighting("a", 20, Country::Germany)]])
            .await
            .unwrap();

        let written = write_in_calls(&root, &[&[sighting("b", 21, Country::Germany)]])
            .await
            .unwrap();

        assert_eq!(written.partitions.removed, 1);
        assert!(!sighting_partition(&tmp, "DE", 20));
        assert!(sighting_partition(&tmp, "DE", 21));
    }

    #[tokio::test]
    async fn a_country_no_call_wrote_is_removed_on_finishing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write_in_calls(&root, &[&[sighting("a", 21, Country::UnitedKingdom)]])
            .await
            .unwrap();

        let written = write_in_calls(&root, &[&[sighting("b", 21, Country::Germany)]])
            .await
            .unwrap();

        assert_eq!(written.partitions.removed, 1);
        assert!(!tmp.path().join("silver/sighting/country=GB").exists());
    }

    #[tokio::test]
    async fn rows_of_one_partition_may_arrive_over_several_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());

        let written = write_in_calls(
            &root,
            &[
                &[sighting("a", 21, Country::Germany)],
                &[sighting("b", 21, Country::Germany)],
            ],
        )
        .await
        .unwrap();

        assert_eq!(written.partitions.written, 1);
        let query = Query::new(root);
        query
            .register(SightingRow::DATASET, "sighting")
            .await
            .unwrap();
        assert_eq!(
            query
                .count("SELECT COUNT(*) AS count FROM sighting")
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn a_geometry_row_lands_under_its_country_holding_the_projection_it_carries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let row = sighting("a", 21, Country::UnitedKingdom);

        write_in_calls(&root, &[std::slice::from_ref(&row)])
            .await
            .unwrap();

        assert!(sighting_partition(&tmp, "GB", 21));
        let query = Query::new(root);
        query
            .register(SightingRow::DATASET, "sighting")
            .await
            .unwrap();
        let batches = query
            .sql(&format!(
                "SELECT ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY} FROM sighting"
            ))
            .await
            .unwrap();
        let projected = crate::geo::geometries(&batches[0], PROJECTED_GEOMETRY).unwrap();
        assert_eq!(
            projected,
            [geo_types::Geometry::Point(*row.geometry.projected())]
        );
    }

    #[tokio::test]
    async fn closing_the_days_before_a_date_lands_their_files_and_keeps_the_rest_open() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let mut writer = GeometryRowWriter::<SightingRow>::new(&root).unwrap();
        writer
            .write(&[
                sighting("a", 21, Country::Germany),
                sighting("b", 22, Country::Germany),
            ])
            .await
            .unwrap();

        writer.close_days_before(at(22).date_naive()).await.unwrap();

        assert!(sighting_partition(&tmp, "DE", 21));
        assert!(!sighting_partition(&tmp, "DE", 22));
        assert_eq!(writer.finish().await.unwrap().partitions.written, 2);
    }

    #[tokio::test]
    async fn a_row_for_a_closed_day_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let mut writer = GeometryRowWriter::<SightingRow>::new(&root).unwrap();
        writer
            .write(&[sighting("a", 21, Country::Germany)])
            .await
            .unwrap();
        writer.close_days_before(at(22).date_naive()).await.unwrap();

        let err = writer
            .write(&[sighting("b", 21, Country::Germany)])
            .await
            .unwrap_err();

        assert!(matches!(err, TableError::DayClosed { .. }), "{err}");
    }

    #[tokio::test]
    async fn rows_named_uniquely_cannot_be_written_over_several_calls() {
        let tmp = tempfile::tempdir().unwrap();

        let err = GeometryRowWriter::<TrackRow>::new(&Root::new(tmp.path()))
            .err()
            .expect("a refusal");

        assert!(
            matches!(err, TableError::UniqueAcrossWrites { .. }),
            "{err}"
        );
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, day, 9, 0, 0).unwrap()
    }

    fn berlin() -> Point<f64> {
        Point::new(13.404954, 52.520008)
    }

    fn track(id: &str, day: u32, country: Country) -> GeographyRow<TrackRow, LineString<f64>> {
        GeographyRow {
            row: TrackRow {
                track_id: id.to_string(),
                seen_at: at(day),
            },
            geography: LineString::from(vec![berlin(), berlin()]),
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

        let written = write_geography_rows(
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

        let written = write_geography_rows(
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
    async fn a_failed_day_leaves_the_days_before_it_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let nowhere = Point::new(f64::NAN, f64::NAN);
        let unprojectable = GeographyRow {
            geography: LineString::from(vec![nowhere, nowhere]),
            ..track("b", 22, Country::Germany)
        };

        let err = write_geography_rows(&root, &[track("a", 21, Country::Germany), unprojectable])
            .await
            .unwrap_err();

        assert!(matches!(err, TableError::Geo(_)), "{err}");
        assert!(
            tmp.path()
                .join("silver/track/country=DE/seen_date=2026-07-21")
                .exists()
        );
    }

    #[tokio::test]
    async fn the_projected_column_holds_the_countrys_metres() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write_geography_rows(&root, &[track("a", 21, Country::Germany)])
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

        let err = write_geography_rows(
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

        let err = write_geography_rows(
            &Root::new(tmp.path()),
            &[GeographyRow {
                row: pass("a", 21),
                geography: berlin(),
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
        write_geography_rows(&root, &[track("a", 21, Country::Germany)])
            .await
            .unwrap();

        let written = write_geography_rows::<TrackRow, LineString<f64>>(&root, &[])
            .await
            .unwrap();

        assert_eq!(written.rows, 0);
        assert_eq!(written.partitions.removed, 1);
        assert!(!tmp.path().join("silver/track/country=DE").exists());
    }
}
