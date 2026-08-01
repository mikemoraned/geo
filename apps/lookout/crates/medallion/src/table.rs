//! Writing a silver dataset from a table built elsewhere.
//!
//! The writers in this workspace hand the store rows of a Rust type. A derivation written in
//! another language cannot, so this is the way in for one: it takes an arrow table, checks it
//! against the dataset's own definition, and writes it through the same replacing primitives
//! a Rust rebuild uses. There is one implementation of the silver format, not one per
//! language.
//!
//! The caller names a dataset and passes rows; everything else follows from the definition:
//!
//! * **Columns** must be exactly the dataset's own, plus its geometry columns, plus the
//!   columns it partitions by. Each is cast to the type the definition states, so which
//!   engine built the table does not change what is stored.
//! * **Geometry** arrives as GeoArrow, in whichever encoding the caller's engine produces,
//!   and is stored as WKB carrying the store's CRS — lat/lon for [`GEOMETRY`], the country's
//!   projected zone for [`PROJECTED_GEOMETRY`].
//! * **Partitions** are read from the columns named by the layout, and are not written into
//!   the file: a partition's value lives in its path.
//!
//! The write replaces the whole dataset, so the table must hold every row of it — the rule
//! silver rebuilds already follow, and the reason a partition the table has no rows for is
//! swept.

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{Array, ArrayRef, AsArray, RecordBatch, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, FieldRef, Schema};
use arrow::row::{RowConverter, SortField};
use arrow::util::display::{ArrayFormatter, FormatOptions};
use chrono::NaiveDate;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::cast::to_wkb;
use geoarrow_schema::error::GeoArrowError;

use crate::country::{COUNTRY, Country, UnknownCountry};
use crate::dataset::DatasetSpec;
use crate::geo::{GEOMETRY, GeoError, PROJECTED_GEOMETRY, projected_wkb_field, wkb_field};
use crate::layer::layers;
use crate::path::{ReplaceError, Replaced, Root};
use crate::rows::{Row, RowError, fields};

/// Whether a dataset carries geometry.
///
/// Silver's geometry columns are named the same across every dataset and are built as arrow
/// rather than traced from a row type, so a dataset says here whether it has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Geometry {
    /// No geometry: the dataset's rows are attributes and identifiers only.
    Absent,
    /// The lat/lon geometry every geo dataset carries, and its metric twin.
    LatLonAndProjected,
}

/// A silver dataset as something a table can be written to: where it lives, the columns it
/// holds, and whether it carries geometry.
///
/// Built from the dataset's own [`Row`] type, so the columns a table is checked against are
/// the ones the definition states rather than a second listing that could drift from it.
#[derive(Debug, Clone)]
pub struct SilverTarget {
    spec: DatasetSpec<layers::Silver>,
    columns: Vec<FieldRef>,
    geometry: Geometry,
    unique: &'static [&'static str],
}

/// How a dataset's partition directories are laid out, and so which columns a table must
/// carry to be split across them.
///
/// This follows from the definition rather than being declared: a dataset carrying projected
/// geometry is partitioned by country, because a file states one CRS for that column and the
/// zone is chosen per country.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// `country=<iso>` — reference-derived geo data, one file per country.
    Country,
    /// `<key>=<date>` — dated rows carrying no geometry.
    Date(&'static str),
    /// `country=<iso>/<key>=<date>` — dated geometry.
    CountryAndDate(&'static str),
}

/// The suffix a date-valued partition key ends with, as `docs/medallion.md` requires.
const DATE_KEY_SUFFIX: &str = "_date";

/// A failure writing a table into a dataset.
#[derive(Debug, thiserror::Error)]
pub enum TableError {
    #[error("{dataset} has no column named `{column}`")]
    Missing {
        dataset: &'static str,
        column: String,
    },
    #[error("{dataset} does not hold the column(s) {columns:?}")]
    Unexpected {
        dataset: &'static str,
        columns: Vec<String>,
    },
    #[error("{dataset}.{column} identifies a row, but rows {first} and {second} both hold {value}")]
    Duplicate {
        dataset: &'static str,
        column: String,
        value: String,
        first: usize,
        second: usize,
    },
    #[error(
        "{dataset}.{column} is {found}, which cannot be read as the {expected} it is stored as"
    )]
    Untranslatable {
        dataset: &'static str,
        column: String,
        found: DataType,
        expected: DataType,
    },
    #[error("{dataset}.{column} holds no date at row {row}")]
    UndatedRow {
        dataset: &'static str,
        column: String,
        row: usize,
    },
    #[error("{dataset}.{column}: {source}")]
    Country {
        dataset: &'static str,
        column: String,
        #[source]
        source: UnknownCountry,
    },
    #[error(
        "{dataset} is partitioned on `{key}`, which is neither `{COUNTRY}` nor a \
         `<event>{DATE_KEY_SUFFIX}` key, so a table cannot be split across its partitions"
    )]
    UnsupportedLayout { dataset: &'static str, key: String },
    #[error("{0} is not partitioned, so a table cannot be split across its partitions")]
    Unpartitioned(&'static str),
    #[error("reading the table: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("reading the geometry: {0}")]
    GeoArrow(#[from] GeoArrowError),
    #[error(transparent)]
    Geo(#[from] GeoError),
    #[error(transparent)]
    Replace(#[from] ReplaceError),
    #[error(transparent)]
    Write(#[from] crate::write::WriteError),
    #[error(transparent)]
    Path(#[from] crate::partition::PathError),
    #[error(transparent)]
    Row(#[from] RowError),
}

/// What writing a table left in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableWritten {
    pub rows: usize,
    pub partitions: Replaced,
}

impl SilverTarget {
    /// The dataset `R`'s rows make up, as somewhere a table can be written.
    pub fn of<R: Row<Layer = layers::Silver>>(geometry: Geometry) -> Result<Self, RowError> {
        Ok(Self {
            spec: R::DATASET,
            columns: fields::<R>()?,
            geometry,
            unique: R::UNIQUE,
        })
    }

    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    /// The columns a table must carry: the dataset's own, its geometry, and the ones its
    /// partitions are read from.
    pub fn expected_columns(&self) -> Result<Vec<String>, TableError> {
        let own = self.columns.iter().map(|field| field.name().clone());
        Ok(own
            .chain(self.geometry_columns().map(str::to_string))
            .chain(self.partition_columns()?.into_iter().map(str::to_string))
            .collect())
    }

    fn geometry_columns(&self) -> impl Iterator<Item = &'static str> {
        match self.geometry {
            Geometry::Absent => [].iter(),
            Geometry::LatLonAndProjected => [GEOMETRY, PROJECTED_GEOMETRY].iter(),
        }
        .copied()
    }

    /// The columns the partition values are read from, outermost first.
    fn partition_columns(&self) -> Result<Vec<&'static str>, TableError> {
        Ok(match self.layout()? {
            Layout::Country => vec![COUNTRY],
            Layout::Date(key) => vec![key],
            Layout::CountryAndDate(key) => vec![COUNTRY, key],
        })
    }

    fn layout(&self) -> Result<Layout, TableError> {
        let key = self
            .spec
            .partition_key
            .ok_or(TableError::Unpartitioned(self.spec.name))?;

        match (key, self.geometry) {
            (COUNTRY, _) => Ok(Layout::Country),
            (key, Geometry::LatLonAndProjected) if key.ends_with(DATE_KEY_SUFFIX) => {
                Ok(Layout::CountryAndDate(key))
            }
            (key, Geometry::Absent) if key.ends_with(DATE_KEY_SUFFIX) => Ok(Layout::Date(key)),
            (key, _) => Err(TableError::UnsupportedLayout {
                dataset: self.spec.name,
                key: key.to_string(),
            }),
        }
    }
}

/// Write `table` as the whole of `target`'s dataset, replacing what is there.
///
/// The batches are read as one table, so a caller streaming a large result still gets one
/// file per partition rather than one per batch — a silver partition is one file.
///
/// A table of no rows is a derivation that produced nothing, and sweeps the dataset away. A
/// call with no batches at all carries no schema to check and is a no-op: emptying a dataset
/// has to be said with a table, not with silence.
pub async fn write_table(
    root: &Root,
    target: &SilverTarget,
    table: &[RecordBatch],
) -> Result<TableWritten, TableError> {
    let Some(first) = table.first() else {
        return Ok(TableWritten::default());
    };
    let table = arrow::compute::concat_batches(&first.schema(), table)?;
    check_columns(target, &table)?;
    check_unique(target, &table)?;

    let columns = translate(target, &table)?;
    let written = match target.layout()? {
        Layout::Country => write_by_country(root, target, &table, &columns, None).await?,
        Layout::Date(key) => {
            let days = days(target, &dates_of(target, &table, key)?, &columns, None)?;
            replace_dates(&root.dataset(target.spec), target, &days).await?
        }
        Layout::CountryAndDate(key) => {
            write_by_country(root, target, &table, &columns, Some(key)).await?
        }
    };

    Ok(TableWritten {
        rows: table.num_rows(),
        partitions: written,
    })
}

/// One country's rows, written as a partition of their own or as dates below it, with the
/// countries the run did not produce swept away.
async fn write_by_country(
    root: &Root,
    target: &SilverTarget,
    table: &RecordBatch,
    columns: &Columns,
    dates: Option<&'static str>,
) -> Result<Replaced, TableError> {
    let by_country = group(&countries_of(target, table)?);
    let derived: Vec<Country> = by_country.iter().map(|(country, _)| *country).collect();
    let dates = dates.map(|key| dates_of(target, table, key)).transpose()?;
    let mut written = Replaced::default();

    for (country, rows) in by_country {
        let dataset = root.dataset(target.spec).partition(COUNTRY, country)?;
        let columns = columns.take(&rows)?;
        written += match &dates {
            None => {
                let batch = [columns.batch(target, Some(country))?];
                match target.geometry {
                    Geometry::Absent => dataset.replace_with(&batch).await.map(|_| ())?,
                    Geometry::LatLonAndProjected => {
                        dataset.replace_with_geo(&batch).await.map(|_| ())?
                    }
                }
                Replaced {
                    written: 1,
                    removed: 0,
                }
            }
            Some(all) => {
                let mine: Vec<NaiveDate> = rows.iter().map(|row| all[*row as usize]).collect();
                let days = days(target, &mine, &columns, Some(country))?;
                replace_dates(&dataset, target, &days).await?
            }
        };
    }

    written.removed += root
        .dataset(target.spec)
        .retain_partitions(COUNTRY, &derived)
        .await?;
    Ok(written)
}

/// One partition per date, written with the encoder the dataset's geometry calls for.
async fn replace_dates(
    dataset: &crate::path::Dataset<layers::Silver>,
    target: &SilverTarget,
    days: &[(NaiveDate, RecordBatch)],
) -> Result<Replaced, TableError> {
    Ok(match target.geometry {
        Geometry::Absent => dataset.replace_dates(days).await?,
        Geometry::LatLonAndProjected => dataset.replace_dates_geo(days).await?,
    })
}

/// One batch per date the rows fall on, in the order the dates first appear. `dates` names
/// the date of each row of `columns`, in the same order.
fn days(
    target: &SilverTarget,
    dates: &[NaiveDate],
    columns: &Columns,
    country: Option<Country>,
) -> Result<Vec<(NaiveDate, RecordBatch)>, TableError> {
    group(dates)
        .into_iter()
        .map(|(date, rows)| Ok((date, columns.take(&rows)?.batch(target, country)?)))
        .collect()
}

/// The rows holding each distinct value, in the order the values first appear.
fn group<T: Copy + Eq + std::hash::Hash>(values: &[T]) -> Vec<(T, Vec<u32>)> {
    let mut order: Vec<T> = Vec::new();
    let mut rows: HashMap<T, Vec<u32>> = HashMap::new();
    for (row, value) in values.iter().enumerate() {
        rows.entry(*value)
            .or_insert_with(|| {
                order.push(*value);
                Vec::new()
            })
            .push(row as u32);
    }
    order
        .into_iter()
        .map(|value| {
            let taken = rows.remove(&value).unwrap_or_default();
            (value, taken)
        })
        .collect()
}

/// The columns as the store holds them: the dataset's own, cast to their defined types, and
/// its geometry as WKB.
///
/// Built once for the whole table and then taken from per partition, since casting and
/// re-encoding are the expensive part and neither depends on how the rows are split up.
struct Columns {
    own: Vec<ArrayRef>,
    geometry: Option<ArrayRef>,
    projected: Option<ArrayRef>,
}

impl Columns {
    /// The same columns holding only `rows`.
    fn take(&self, rows: &[u32]) -> Result<Self, TableError> {
        let indices = UInt32Array::from(rows.to_vec());
        let taken = |array: &ArrayRef| arrow::compute::take(array, &indices, None);
        Ok(Self {
            own: self
                .own
                .iter()
                .map(taken)
                .collect::<Result<Vec<_>, arrow::error::ArrowError>>()?,
            geometry: self.geometry.as_ref().map(taken).transpose()?,
            projected: self.projected.as_ref().map(taken).transpose()?,
        })
    }

    /// One partition's batch: the dataset's columns, then its geometry with the CRS the
    /// store states for it.
    ///
    /// `country` fixes the projected column's zone, and is present whenever the dataset
    /// carries one — that is what its partitioning is for.
    fn batch(
        &self,
        target: &SilverTarget,
        country: Option<Country>,
    ) -> Result<RecordBatch, TableError> {
        let mut fields: Vec<FieldRef> = target.columns.clone();
        let mut arrays: Vec<ArrayRef> = self.own.clone();

        if let Some(geometry) = &self.geometry {
            fields.push(Arc::new(wkb_field(GEOMETRY)?));
            arrays.push(geometry.clone());
        }
        if let (Some(projected), Some(country)) = (&self.projected, country) {
            fields.push(Arc::new(projected_wkb_field(PROJECTED_GEOMETRY, country)?));
            arrays.push(projected.clone());
        }

        Ok(RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)?)
    }
}

/// The table's columns as the store holds them, failing where one cannot be read as its
/// defined type.
fn translate(target: &SilverTarget, table: &RecordBatch) -> Result<Columns, TableError> {
    let own = target
        .columns
        .iter()
        .map(|field| cast(target, table, field.name(), field.data_type()))
        .collect::<Result<Vec<_>, TableError>>()?;

    let geometry = match target.geometry {
        Geometry::Absent => (None, None),
        Geometry::LatLonAndProjected => (
            Some(wkb(target, table, GEOMETRY)?),
            Some(wkb(target, table, PROJECTED_GEOMETRY)?),
        ),
    };

    Ok(Columns {
        own,
        geometry: geometry.0,
        projected: geometry.1,
    })
}

/// One geometry column as WKB, whatever GeoArrow encoding it arrived in.
///
/// A binary column declaring no encoding is taken to be WKB already, which is what an engine
/// that has no geometry type of its own produces. Anything else is read as the GeoArrow
/// extension type it declares and re-encoded.
///
/// The CRS the column claims is not read: silver states the CRS of each of its geometry
/// columns, so the coordinates are taken to be in it and the field is stamped accordingly.
fn wkb(target: &SilverTarget, table: &RecordBatch, column: &str) -> Result<ArrayRef, TableError> {
    let (index, field) = column_of(target, table, column)?;
    let array = table.column(index);

    let undeclared = field.extension_type_name().is_none();
    let binary = matches!(
        field.data_type(),
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView
    );
    if undeclared && binary {
        return Ok(arrow::compute::cast(array, &DataType::Binary)?);
    }

    let geometry = geoarrow_array::array::from_arrow_array(array, field)?;
    let wkb = to_wkb::<i32>(geometry.as_ref())?;
    Ok(wkb.to_array_ref())
}

/// One column read as `expected`.
fn cast(
    target: &SilverTarget,
    table: &RecordBatch,
    column: &str,
    expected: &DataType,
) -> Result<ArrayRef, TableError> {
    let (index, field) = column_of(target, table, column)?;
    let array = table.column(index);
    if field.data_type() == expected {
        return Ok(array.clone());
    }
    if !arrow::compute::can_cast_types(field.data_type(), expected) {
        return Err(TableError::Untranslatable {
            dataset: target.name(),
            column: column.to_string(),
            found: field.data_type().clone(),
            expected: expected.clone(),
        });
    }
    Ok(arrow::compute::cast(array, expected)?)
}

/// One named column of the table, or a failure naming the dataset it is missing from.
fn column_of<'a>(
    target: &SilverTarget,
    table: &'a RecordBatch,
    column: &str,
) -> Result<(usize, &'a Field), TableError> {
    let index = table
        .schema_ref()
        .index_of(column)
        .map_err(|_| TableError::Missing {
            dataset: target.name(),
            column: column.to_string(),
        })?;
    Ok((index, table.schema_ref().field(index)))
}

/// The country each row belongs to, read from the column the layout names.
fn countries_of(target: &SilverTarget, table: &RecordBatch) -> Result<Vec<Country>, TableError> {
    let codes = cast(target, table, COUNTRY, &DataType::Utf8)?;
    let codes = codes
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| TableError::Missing {
            dataset: target.name(),
            column: COUNTRY.to_string(),
        })?;

    (0..codes.len())
        .map(|row| {
            codes
                .value(row)
                .parse::<Country>()
                .map_err(|source| TableError::Country {
                    dataset: target.name(),
                    column: COUNTRY.to_string(),
                    source,
                })
        })
        .collect()
}

/// The date each row belongs to, read from the column the layout names.
fn dates_of(
    target: &SilverTarget,
    table: &RecordBatch,
    key: &'static str,
) -> Result<Vec<NaiveDate>, TableError> {
    let dates = cast(target, table, key, &DataType::Date32)?;
    let dates = dates.as_primitive::<arrow::datatypes::Date32Type>();

    (0..dates.len())
        .map(|row| {
            (!dates.is_null(row))
                .then(|| arrow::temporal_conversions::date32_to_datetime(dates.value(row)))
                .flatten()
                .map(|at| at.date())
                .ok_or_else(|| TableError::UndatedRow {
                    dataset: target.name(),
                    column: key.to_string(),
                    row,
                })
        })
        .collect()
}


/// Refuse a table two of whose rows share a value in a column the dataset declares unique.
///
/// Checked over the whole table rather than per partition, because a name that identifies a
/// row has to do so across the dataset — the partition a row lands in is a fact about how it
/// is stored, not about what it is called.
fn check_unique(target: &SilverTarget, table: &RecordBatch) -> Result<(), TableError> {
    for column in target.unique {
        let array = table
            .column_by_name(column)
            .ok_or_else(|| TableError::Missing {
                dataset: target.name(),
                column: column.to_string(),
            })?;
        let converter = RowConverter::new(vec![SortField::new(array.data_type().clone())])?;
        let encoded = converter.convert_columns(std::slice::from_ref(array))?;

        let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
        for row in 0..array.len() {
            if let Some(first) = seen.insert(encoded.row(row).as_ref().to_vec(), row) {
                let shown = ArrayFormatter::try_new(array, &FormatOptions::default())?;
                return Err(TableError::Duplicate {
                    dataset: target.name(),
                    column: column.to_string(),
                    value: shown.value(row).to_string(),
                    first,
                    second: row,
                });
            }
        }
    }

    Ok(())
}

/// Fail unless the table's columns are exactly the dataset's.
///
/// A missing column is named on its own, since that is what a caller has to add; extra
/// columns are reported together, since a table built by a query usually carries several
/// working columns the dataset does not hold.
fn check_columns(target: &SilverTarget, table: &RecordBatch) -> Result<(), TableError> {
    let expected = target.expected_columns()?;
    for column in &expected {
        if table.schema_ref().index_of(column).is_err() {
            return Err(TableError::Missing {
                dataset: target.name(),
                column: column.clone(),
            });
        }
    }

    let unexpected: Vec<String> = table
        .schema_ref()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .filter(|name| !expected.contains(name))
        .collect();
    if !unexpected.is_empty() {
        return Err(TableError::Unexpected {
            dataset: target.name(),
            columns: unexpected,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use arrow::array::{BinaryArray, Date32Array, TimestampMillisecondArray};
    use arrow::datatypes::TimeUnit;
    use geo_types::{LineString, Point};
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::geo::{Projector, wkb_column};
    use crate::path::Root;
    use crate::query::Query;

    /// Reference-derived geometry: one partition per country.
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct CrossingRow {
        crossing_id: String,
        overlap_m: f64,
    }

    impl Row for CrossingRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("crossing", "country");
        const UNIQUE: &'static [&'static str] = &["crossing_id"];
    }

    /// Dated observations carrying no geometry: one partition per date.
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct PassRow {
        crossing_id: String,
        crossed_at: i64,
    }

    impl Row for PassRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("pass", "crossed_date");
        const INSTANTS: &'static [&'static str] = &["crossed_at"];
    }

    /// Dated geometry: a country partition above the date, since the file states one CRS.
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TrackRow {
        track_id: String,
    }

    impl Row for TrackRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("track", "seen_date");
    }

    fn crossings() -> SilverTarget {
        SilverTarget::of::<CrossingRow>(Geometry::LatLonAndProjected).unwrap()
    }

    /// A table shaped like `crossing`: the row columns, both geometries, and the country.
    fn crossing_table(ids: &[&str], points: &[Point<f64>], countries: &[&str]) -> RecordBatch {
        let projector = Projector::for_country(Country::Germany).unwrap();
        let projected: Vec<Point<f64>> = points
            .iter()
            .map(|point| projector.project(point).unwrap())
            .collect();
        let (geometry_field, geometry) = wkb_column(wkb_field(GEOMETRY).unwrap(), points).unwrap();
        let (projected_field, projected) = wkb_column(
            projected_wkb_field(PROJECTED_GEOMETRY, Country::Germany).unwrap(),
            &projected,
        )
        .unwrap();

        RecordBatch::try_from_iter(vec![
            (
                "crossing_id",
                Arc::new(StringArray::from(ids.to_vec())) as ArrayRef,
            ),
            (
                "overlap_m",
                Arc::new(arrow::array::Float64Array::from(vec![1.5; ids.len()])) as ArrayRef,
            ),
            (GEOMETRY, geometry.clone()),
            (PROJECTED_GEOMETRY, projected.clone()),
            (
                COUNTRY,
                Arc::new(StringArray::from(countries.to_vec())) as ArrayRef,
            ),
        ])
        .unwrap()
        .with_schema(Arc::new(Schema::new(vec![
            Arc::new(Field::new("crossing_id", DataType::Utf8, false)),
            Arc::new(Field::new("overlap_m", DataType::Float64, false)),
            geometry_field,
            projected_field,
            Arc::new(Field::new(COUNTRY, DataType::Utf8, false)),
        ])))
        .unwrap()
    }

    /// A date as the Date32 an engine hands one over as.
    fn epoch_day(date: &str) -> i32 {
        let date: NaiveDate = date.parse().unwrap();
        (date - NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days() as i32
    }

    fn berlin() -> Point<f64> {
        Point::new(13.404954, 52.520008)
    }

    fn frankfurt() -> Point<f64> {
        Point::new(8.682127, 50.110924)
    }

    #[tokio::test]
    async fn a_table_lands_in_one_file_per_country() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let table = crossing_table(&["a", "b"], &[berlin(), frankfurt()], &["DE", "DE"]);

        let written = write_table(&root, &crossings(), &[table]).await.unwrap();

        assert_eq!(written.rows, 2);
        assert_eq!(written.partitions.written, 1);
        assert!(
            tmp.path()
                .join("silver/crossing/country=DE/part-0.parquet")
                .exists()
        );
    }

    /// The value of a partition lives in its path, so the column it was read from is not
    /// also written into the file — a reader gets it back from the path either way.
    #[tokio::test]
    async fn the_partition_column_is_not_stored_in_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let table = crossing_table(&["a"], &[berlin()], &["DE"]);
        write_table(&root, &crossings(), &[table]).await.unwrap();

        let query = Query::new(root);
        query
            .register(CrossingRow::DATASET, "crossing")
            .await
            .unwrap();
        let batches = query.sql("SELECT * FROM crossing").await.unwrap();

        let columns: Vec<&str> = batches[0]
            .schema_ref()
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect();
        // `country` comes back, but as the discovered partition key rather than as a
        // column of the file: it is the last one, after everything the file holds.
        assert_eq!(
            columns,
            vec![
                "crossing_id",
                "overlap_m",
                GEOMETRY,
                PROJECTED_GEOMETRY,
                COUNTRY
            ]
        );
    }

    /// What a notebook actually reads back: the coordinates it handed over, not something
    /// the WKB round trip moved.
    #[tokio::test]
    async fn the_geometry_reads_back_as_it_was_handed_over() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let table = crossing_table(&["a"], &[berlin()], &["DE"]);
        write_table(&root, &crossings(), &[table]).await.unwrap();

        let query = Query::new(root);
        query
            .register(CrossingRow::DATASET, "crossing")
            .await
            .unwrap();
        let batches = query
            .sql(&format!(
                "SELECT {GEOMETRY}, {PROJECTED_GEOMETRY} FROM crossing"
            ))
            .await
            .unwrap();

        let lat_lon = crate::geo::geometries(&batches[0], GEOMETRY).unwrap();
        let projected = crate::geo::geometries(&batches[0], PROJECTED_GEOMETRY).unwrap();
        assert_eq!(lat_lon, vec![geo_types::Geometry::Point(berlin())]);
        let geo_types::Geometry::Point(metres) = projected[0] else {
            panic!("expected a point, got {:?}", projected[0]);
        };
        assert!((metres.x() - 798_809.63).abs() < 0.01, "{metres:?}");
    }

    /// A geometry column that declares no encoding is taken as WKB, which is what an engine
    /// with no geometry type of its own hands over.
    #[tokio::test]
    async fn a_plain_binary_geometry_column_is_read_as_wkb() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let declared = crossing_table(&["a"], &[berlin()], &["DE"]);
        let undeclared = RecordBatch::try_from_iter(
            declared
                .schema_ref()
                .fields()
                .iter()
                .zip(declared.columns())
                .map(|(field, column)| (field.name().as_str(), column.clone())),
        )
        .unwrap();

        write_table(&root, &crossings(), &[undeclared])
            .await
            .unwrap();

        let query = Query::new(root);
        query
            .register(CrossingRow::DATASET, "crossing")
            .await
            .unwrap();
        let batches = query
            .sql(&format!("SELECT {GEOMETRY} FROM crossing"))
            .await
            .unwrap();
        assert_eq!(
            crate::geo::geometries(&batches[0], GEOMETRY).unwrap(),
            vec![geo_types::Geometry::Point(berlin())]
        );
    }

    /// The types a query engine happens to produce are not the types the store holds, so a
    /// column that can be read as its defined type is.
    #[tokio::test]
    async fn a_column_of_another_type_is_read_as_the_one_the_dataset_defines() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let table = crossing_table(&["a"], &[berlin()], &["DE"]);
        let views: Vec<ArrayRef> = table
            .columns()
            .iter()
            .map(|column| match column.data_type() {
                DataType::Utf8 => arrow::compute::cast(column, &DataType::Utf8View).unwrap(),
                _ => column.clone(),
            })
            .collect();
        let viewed = RecordBatch::try_from_iter(
            table
                .schema_ref()
                .fields()
                .iter()
                .zip(views)
                .map(|(field, column)| (field.name().as_str(), column)),
        )
        .unwrap();

        write_table(&root, &crossings(), &[viewed]).await.unwrap();

        let query = Query::new(root);
        query
            .register(CrossingRow::DATASET, "crossing")
            .await
            .unwrap();
        let rows: Vec<CrossingRow> = query
            .rows("SELECT crossing_id, overlap_m FROM crossing")
            .await
            .unwrap();
        assert_eq!(rows[0].crossing_id, "a");
    }

    #[tokio::test]
    async fn a_missing_column_names_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut without = crossing_table(&["a"], &[berlin()], &["DE"]);
        without.remove_column(1);

        let err = write_table(&Root::new(tmp.path()), &crossings(), &[without])
            .await
            .unwrap_err();

        assert!(
            matches!(&err, TableError::Missing { column, .. } if column == "overlap_m"),
            "{err}"
        );
    }

    /// A table built by a query usually carries working columns, and storing them would put
    /// columns in the dataset that its definition does not have.
    #[tokio::test]
    async fn columns_the_dataset_does_not_hold_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let table = crossing_table(&["a"], &[berlin()], &["DE"]);
        let extra = RecordBatch::try_from_iter(
            table
                .schema_ref()
                .fields()
                .iter()
                .zip(table.columns())
                .map(|(field, column)| (field.name().as_str(), column.clone()))
                .chain([(
                    "scratch",
                    Arc::new(StringArray::from(vec!["x"])) as ArrayRef,
                )]),
        )
        .unwrap();

        let err = write_table(&Root::new(tmp.path()), &crossings(), &[extra])
            .await
            .unwrap_err();

        assert!(
            matches!(&err, TableError::Unexpected { columns, .. } if columns == &["scratch"]),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_country_the_store_does_not_know_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let table = crossing_table(&["a"], &[berlin()], &["ZZ"]);

        let err = write_table(&Root::new(tmp.path()), &crossings(), &[table])
            .await
            .unwrap_err();

        assert!(matches!(err, TableError::Country { .. }), "{err}");
    }

    /// A rebuild replaces the whole dataset, so a partition it no longer produces rows for
    /// goes — the rule silver already follows for a rebuild written in Rust.
    #[tokio::test]
    async fn a_partition_the_table_no_longer_covers_is_swept() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let target = SilverTarget::of::<PassRow>(Geometry::Absent).unwrap();
        let both = pass_table(&["a", "b"], &["2026-07-21", "2026-07-22"]);
        write_table(&root, &target, &[both]).await.unwrap();

        let one = pass_table(&["a"], &["2026-07-21"]);
        let written = write_table(&root, &target, &[one]).await.unwrap();

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

    /// A table shaped like `pass`: no geometry, dated by a column of its own.
    fn pass_table(ids: &[&str], dates: &[&str]) -> RecordBatch {
        let days: Vec<i32> = dates.iter().copied().map(epoch_day).collect();
        RecordBatch::try_from_iter(vec![
            (
                "crossing_id",
                Arc::new(StringArray::from(ids.to_vec())) as ArrayRef,
            ),
            (
                "crossed_at",
                Arc::new(
                    TimestampMillisecondArray::from(vec![1_700_000_000_000i64; ids.len()])
                        .with_timezone("UTC"),
                ) as ArrayRef,
            ),
            (
                "crossed_date",
                Arc::new(Date32Array::from(days)) as ArrayRef,
            ),
        ])
        .unwrap()
    }

    #[test]
    fn a_dated_dataset_carrying_geometry_partitions_by_country_first() {
        let target = SilverTarget::of::<TrackRow>(Geometry::LatLonAndProjected).unwrap();

        assert_eq!(
            target.partition_columns().unwrap(),
            vec![COUNTRY, "seen_date"]
        );
    }

    #[test]
    fn a_dated_dataset_without_geometry_partitions_by_date_alone() {
        let target = SilverTarget::of::<PassRow>(Geometry::Absent).unwrap();

        assert_eq!(target.partition_columns().unwrap(), vec!["crossed_date"]);
    }

    /// The columns a caller has to supply are the dataset's own plus what the layout needs,
    /// which is what the error messages hold them to.
    #[test]
    fn the_expected_columns_are_the_definitions_plus_geometry_and_partitions() {
        assert_eq!(
            crossings().expected_columns().unwrap(),
            vec![
                "crossing_id",
                "overlap_m",
                GEOMETRY,
                PROJECTED_GEOMETRY,
                COUNTRY
            ]
        );
    }

    #[tokio::test]
    async fn a_dated_geometry_table_lands_under_its_country() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let target = SilverTarget::of::<TrackRow>(Geometry::LatLonAndProjected).unwrap();
        let projector = Projector::for_country(Country::Germany).unwrap();
        let line = LineString::from(vec![berlin().0, frankfurt().0]);
        let projected = projector.project(&line).unwrap();
        let (geometry_field, geometry) = wkb_column(wkb_field(GEOMETRY).unwrap(), &[line]).unwrap();
        let (projected_field, projected_array) = wkb_column(
            projected_wkb_field(PROJECTED_GEOMETRY, Country::Germany).unwrap(),
            &[projected],
        )
        .unwrap();
        let table = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Arc::new(Field::new("track_id", DataType::Utf8, false)),
                geometry_field,
                projected_field,
                Arc::new(Field::new(COUNTRY, DataType::Utf8, false)),
                Arc::new(Field::new("seen_date", DataType::Date32, false)),
            ])),
            vec![
                Arc::new(StringArray::from(vec!["t"])) as ArrayRef,
                geometry,
                projected_array,
                Arc::new(StringArray::from(vec!["DE"])) as ArrayRef,
                Arc::new(Date32Array::from(vec![epoch_day("2026-07-21")])) as ArrayRef,
            ],
        )
        .unwrap();

        write_table(&root, &target, &[table]).await.unwrap();

        assert!(
            tmp.path()
                .join("silver/track/country=DE/seen_date=2026-07-21/part-0.parquet")
                .exists()
        );
    }

    /// The instant columns of a dataset stay instants: a table handing them over as
    /// microseconds is read as the milliseconds the store holds.
    #[tokio::test]
    async fn an_instant_column_keeps_its_defined_unit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let target = SilverTarget::of::<PassRow>(Geometry::Absent).unwrap();
        let table = pass_table(&["a"], &["2026-07-21"]);
        let micros = arrow::compute::cast(
            table.column_by_name("crossed_at").unwrap(),
            &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        )
        .unwrap();
        let table = RecordBatch::try_from_iter(vec![
            ("crossing_id", table.column(0).clone()),
            ("crossed_at", micros),
            ("crossed_date", table.column(2).clone()),
        ])
        .unwrap();

        write_table(&root, &target, &[table]).await.unwrap();

        let query = Query::new(root);
        query.register(PassRow::DATASET, "pass").await.unwrap();
        let batches = query.sql("SELECT crossed_at FROM pass").await.unwrap();
        assert_eq!(
            batches[0].schema_ref().field(0).data_type(),
            &DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()))
        );
    }

    /// Nothing about the store's own writers changes here: a geometry column arrives as
    /// binary WKB either way, and this is the check that the two agree.
    #[test]
    fn the_wkb_a_table_carries_is_the_wkb_the_store_writes() {
        let (_, from_geometry) = wkb_column(wkb_field(GEOMETRY).unwrap(), &[berlin()]).unwrap();
        let table = crossing_table(&["a"], &[berlin()], &["DE"]);
        let target = crossings();

        let translated = translate(&target, &table).unwrap();

        let expected = from_geometry
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap();
        let actual = translated
            .geometry
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap();
        assert_eq!(actual.value(0), expected.value(0));
    }

    /// An id that named two rows would let a reader take one row for another, and would let a
    /// device holding it point at either — so the write is refused rather than stored.
    #[tokio::test]
    async fn a_table_naming_two_rows_the_same_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let table = crossing_table(
            &["a", "b", "a"],
            &[berlin(), berlin(), berlin()],
            &["DE"; 3],
        );

        let err = write_table(&root, &crossings(), &[table])
            .await
            .unwrap_err();

        assert!(
            matches!(&err, TableError::Duplicate { column, value, first: 0, second: 2, .. }
                if column == "crossing_id" && value == "a"),
            "{err:?}"
        );
    }

    /// The rule is over the dataset, not over a partition of it: two rows of one name are two
    /// rows of one name however they are laid out.
    #[tokio::test]
    async fn a_repeated_id_is_refused_even_across_partitions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        let table = crossing_table(&["a", "a"], &[berlin(), berlin()], &["DE", "FR"]);

        assert!(matches!(
            write_table(&root, &crossings(), &[table]).await,
            Err(TableError::Duplicate { .. })
        ));
    }
}
