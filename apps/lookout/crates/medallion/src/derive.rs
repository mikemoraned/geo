use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::marker::PhantomData;

use arrow::array::RecordBatch;
use chrono::NaiveDate;

use crate::country::{COUNTRY, Country};
use crate::geo::{GeoParquetFile, Projector, geo_batch, geometry_fields};
use crate::layer::layers;
use crate::partition::DATE_FORMAT;
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
    let mut writer = GeoRowWriter::<R>::for_any_rows(root)?;
    check_named(&writer.target, rows.iter().map(|placed| &placed.row))?;
    writer.write(rows).await?;
    writer.finish().await
}

pub struct GeoRowWriter<R> {
    root: Root,
    target: SilverTarget,
    date_key: &'static str,
    projectors: HashMap<Country, Projector>,
    open: HashMap<(Country, NaiveDate), GeoParquetFile>,
    rows: usize,
    row: PhantomData<R>,
}

impl<R> GeoRowWriter<R>
where
    R: Dated<Layer = layers::Silver> + Clone,
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
            projectors: HashMap::new(),
            open: HashMap::new(),
            rows: 0,
            row: PhantomData,
        })
    }

    pub async fn write<G>(&mut self, rows: &[GeoRow<R, G>]) -> Result<(), TableError>
    where
        G: geo_traits::GeometryTrait<T = f64> + geo::MapCoords<f64, f64, Output = G> + Clone,
    {
        let partition_of_each_row: Vec<(Country, NaiveDate)> = rows
            .iter()
            .map(|placed| (placed.country, placed.row.partition_date()))
            .collect();

        for ((country, date), indices) in group(&partition_of_each_row) {
            let projector = match self.projectors.entry(country) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(Projector::for_country(country)?),
            };
            let day: Vec<&GeoRow<R, G>> = indices.iter().map(|row| &rows[*row as usize]).collect();
            let batch = geo_day(&day, projector, country)?;
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
        }

        self.rows += rows.len();
        Ok(())
    }

    pub async fn finish(self) -> Result<TableWritten, TableError> {
        let mut dates_by_country: HashMap<Country, Vec<String>> = HashMap::new();
        for ((country, date), file) in self.open {
            file.close().await?;
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

    let [plain, projected_field] = geometry_fields(country)?;
    Ok(geo_batch(
        &rows,
        &[
            (plain, geometry.as_slice()),
            (projected_field, projected.as_slice()),
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
    use crate::geo::PROJECTED_GEOMETRY;
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

    fn sighting(id: &str, day: u32, country: Country) -> GeoRow<SightingRow, Point<f64>> {
        GeoRow {
            row: SightingRow {
                track_id: id.to_string(),
                seen_at: at(day),
            },
            geometry: berlin(),
            country,
        }
    }

    async fn write_in_calls(
        root: &Root,
        calls: &[&[GeoRow<SightingRow, Point<f64>>]],
    ) -> Result<TableWritten, TableError> {
        let mut writer = GeoRowWriter::<SightingRow>::new(root)?;
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
    async fn rows_named_uniquely_cannot_be_written_over_several_calls() {
        let tmp = tempfile::tempdir().unwrap();

        let err = GeoRowWriter::<TrackRow>::new(&Root::new(tmp.path()))
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
