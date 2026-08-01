//! Read crossings out of the silver GeoParquet.
//!
//! Position is taken from the WKB geometry column rather than from any plain `lat`/`lon`
//! columns a particular export happens to carry, because the geometry is the one place every
//! version of this dataset keeps it.

use std::fs::File;
use std::path::Path;

use arrow::array::{Array, BinaryArray, Float64Array, RecordBatch, StringArray};
use arrow::compute::cast;
use arrow::datatypes::DataType;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};
use geo_types::{Coord, coord};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

/// The stretch of track and the body of water whose meeting *is* the crossing. Read as the
/// source's own identifiers, so the packed id stays a decision made downstream of this.
const IDENTITY: [&str; 2] = [RAIL_ID, WATER_ID];
const RAIL_ID: &str = "rail_id";
const WATER_ID: &str = "water_id";
const GEOMETRY: &str = "geometry";
/// Where along the track the crossing sits. Part of a crossing's identity, because one body
/// of water can meet the same stretch of track many times over — a meandering river crosses
/// a single segment more than a dozen times in this dataset.
const FRAC: &str = "frac";

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("opening {path}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error(transparent)]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("no `{0}` column")]
    MissingColumn(String),
    #[error("`{column}` holds {actual}, which is not readable as {expected}")]
    UnreadableColumn {
        column: String,
        expected: DataType,
        actual: DataType,
    },
    #[error("row {row} has no {column}")]
    Null { row: usize, column: String },
    #[error("row {row}'s geometry is not valid WKB: {source}")]
    Wkb {
        row: usize,
        #[source]
        source: wkb::error::WkbError,
    },
    /// The buffer holds one coordinate per crossing, so anything with an extent has already
    /// been collapsed to a representative point by the time it reaches here.
    #[error("row {row}'s geometry is not a point")]
    NotAPoint { row: usize },
}

/// One crossing, as silver holds it: what meets what, where along the track, and where on
/// the globe.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub rail_id: String,
    pub water_id: String,
    /// How far along `rail_id` the crossing sits, from 0 at its start to 1 at its end.
    pub frac: f64,
    /// Longitude in `x`, latitude in `y`, in WGS84 degrees.
    pub position: Coord<f64>,
}

/// Every crossing in the file, in the order it stores them.
pub fn read(path: &Path) -> Result<Vec<Crossing>, ReadError> {
    let file = File::open(path).map_err(|source| ReadError::Open {
        path: path.display().to_string(),
        source,
    })?;

    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;

    let mut crossings = Vec::new();
    for batch in reader {
        read_batch(&batch?, &mut crossings)?;
    }
    Ok(crossings)
}

/// Appends one batch's crossings, so a row's index is reported against the file rather than
/// against whichever batch it landed in.
fn read_batch(batch: &RecordBatch, crossings: &mut Vec<Crossing>) -> Result<(), ReadError> {
    let first_row = crossings.len();

    let identity = IDENTITY
        .iter()
        .map(|column| strings(batch, column))
        .collect::<Result<Vec<_>, _>>()?;
    let [rail_ids, water_ids] = identity
        .iter()
        .collect::<Vec<_>>()
        .try_into()
        .expect("one array per identity column, just collected");
    let geometries = geometries(batch)?;
    let fracs = fracs(batch)?;

    for row in 0..batch.num_rows() {
        let at = |column: &str| ReadError::Null {
            row: first_row + row,
            column: column.to_string(),
        };
        crossings.push(Crossing {
            rail_id: rail_ids
                .is_valid(row)
                .then(|| rail_ids.value(row).to_string())
                .ok_or_else(|| at(RAIL_ID))?,
            water_id: water_ids
                .is_valid(row)
                .then(|| water_ids.value(row).to_string())
                .ok_or_else(|| at(WATER_ID))?,
            frac: fracs
                .is_valid(row)
                .then(|| fracs.value(row))
                .ok_or_else(|| at(FRAC))?,
            position: geometries
                .is_valid(row)
                .then(|| point(geometries.value(row), first_row + row))
                .ok_or_else(|| at(GEOMETRY))??,
        });
    }
    Ok(())
}

/// The single coordinate of a WKB point.
fn point(geometry: &[u8], row: usize) -> Result<Coord<f64>, ReadError> {
    let geometry =
        wkb::reader::read_wkb(geometry).map_err(|source| ReadError::Wkb { row, source })?;

    let GeometryType::Point(point) = geometry.as_type() else {
        return Err(ReadError::NotAPoint { row });
    };
    // WKB can carry an empty point, which has no coordinate to pack.
    let position = point.coord().ok_or(ReadError::NotAPoint { row })?;

    Ok(coord! { x: position.x(), y: position.y() })
}

/// Casting first means a column is read by what it holds rather than by which width the
/// writer chose: geopandas emits `large_string`/`binary`, arrow's own writers `string`, and
/// either is the same value.
fn strings(batch: &RecordBatch, column: &str) -> Result<StringArray, ReadError> {
    Ok(cast_column(batch, column, &DataType::Utf8)?
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("cast to Utf8 yields a StringArray")
        .clone())
}

fn fracs(batch: &RecordBatch) -> Result<Float64Array, ReadError> {
    Ok(cast_column(batch, FRAC, &DataType::Float64)?
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("cast to Float64 yields a Float64Array")
        .clone())
}

fn geometries(batch: &RecordBatch) -> Result<BinaryArray, ReadError> {
    Ok(cast_column(batch, GEOMETRY, &DataType::Binary)?
        .as_any()
        .downcast_ref::<BinaryArray>()
        .expect("cast to Binary yields a BinaryArray")
        .clone())
}

fn cast_column(
    batch: &RecordBatch,
    column: &str,
    to: &DataType,
) -> Result<arrow::array::ArrayRef, ReadError> {
    let array = batch
        .column_by_name(column)
        .ok_or_else(|| ReadError::MissingColumn(column.to_string()))?;

    cast(array, to).map_err(|_| ReadError::UnreadableColumn {
        column: column.to_string(),
        expected: to.clone(),
        actual: array.data_type().clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Float64Array, LargeStringArray};
    use geo_types::{Geometry, LineString, Point};
    use wkb::writer::{WriteOptions, write_geometry};

    use super::*;

    /// A crossing near Ruhland, of the shape the notebook exports.
    const RAIL: &str = "86aaefea-fac9-4b5e-9f60-2c19678f07c6";
    const WATER: &str = "e597395d-c46d-3b24-a45f-e85abefc2fb5";
    const LON: f64 = 13.548209;
    const LAT: f64 = 51.617567;
    const FRACTION: f64 = 0.128044;

    fn wkb(geometry: &Geometry<f64>) -> Vec<u8> {
        let mut written = Vec::new();
        write_geometry(&mut written, geometry, &WriteOptions::default()).expect("write WKB");
        written
    }

    /// A batch in the exporter's own column types: ids as `large_string`, geometry as
    /// `binary`.
    fn batch_of(geometries: Vec<Option<Vec<u8>>>) -> RecordBatch {
        let rows = geometries.len();
        let ids = |id: &str| LargeStringArray::from(vec![id; rows]);
        let geometries = BinaryArray::from(
            geometries
                .iter()
                .map(|geometry| geometry.as_deref())
                .collect::<Vec<_>>(),
        );

        RecordBatch::try_from_iter(vec![
            (RAIL_ID, Arc::new(ids(RAIL)) as _),
            (WATER_ID, Arc::new(ids(WATER)) as _),
            (
                FRAC,
                Arc::new(Float64Array::from(vec![FRACTION; rows])) as _,
            ),
            (GEOMETRY, Arc::new(geometries) as _),
        ])
        .expect("build a batch")
    }

    fn point_batch() -> RecordBatch {
        batch_of(vec![Some(wkb(&Point::new(LON, LAT).into()))])
    }

    fn read_one(batch: &RecordBatch) -> Result<Vec<Crossing>, ReadError> {
        let mut crossings = Vec::new();
        read_batch(batch, &mut crossings)?;
        Ok(crossings)
    }

    #[test]
    fn a_crossing_is_read_from_its_geometry() {
        let crossings = read_one(&point_batch()).unwrap();

        assert_eq!(
            crossings,
            vec![Crossing {
                rail_id: RAIL.to_string(),
                water_id: WATER.to_string(),
                frac: FRACTION,
                position: coord! { x: LON, y: LAT },
            }]
        );
    }

    /// Some exports carry plain `lat`/`lon` columns beside the geometry, and the silver
    /// dataset keeps position only in the geometry — so a reader that preferred those columns
    /// would break on the dataset that matters. Here they disagree, and the geometry wins.
    #[test]
    fn position_comes_from_the_geometry_not_from_any_lat_lon_columns() {
        let batch = RecordBatch::try_from_iter(vec![
            (RAIL_ID, Arc::new(LargeStringArray::from(vec![RAIL])) as _),
            (WATER_ID, Arc::new(LargeStringArray::from(vec![WATER])) as _),
            (FRAC, Arc::new(Float64Array::from(vec![FRACTION])) as _),
            (
                GEOMETRY,
                Arc::new(BinaryArray::from_vec(vec![&wkb(
                    &Point::new(LON, LAT).into()
                )])) as _,
            ),
            ("lon", Arc::new(Float64Array::from(vec![0.0])) as _),
            ("lat", Arc::new(Float64Array::from(vec![0.0])) as _),
        ])
        .unwrap();

        assert_eq!(
            read_one(&batch).unwrap()[0].position,
            coord! { x: LON, y: LAT }
        );
    }

    #[test]
    fn every_row_of_every_batch_is_read() {
        let point = wkb(&Point::new(LON, LAT).into());
        let batch = batch_of(vec![Some(point.clone()), Some(point.clone()), Some(point)]);

        let mut crossings = Vec::new();
        read_batch(&batch, &mut crossings).unwrap();
        read_batch(&batch, &mut crossings).unwrap();

        assert_eq!(crossings.len(), 6);
    }

    #[test]
    fn a_geometry_with_an_extent_is_rejected() {
        let line = LineString::from(vec![(LON, LAT), (LON + 0.01, LAT)]);
        let batch = batch_of(vec![Some(wkb(&line.into()))]);

        assert!(matches!(
            read_one(&batch),
            Err(ReadError::NotAPoint { row: 0 })
        ));
    }

    #[test]
    fn a_row_is_reported_by_its_position_in_the_file_not_in_its_batch() {
        let batch = batch_of(vec![Some(wkb(&LineString::new(vec![]).into()))]);

        let mut crossings = read_one(&point_batch()).unwrap();

        assert!(matches!(
            read_batch(&batch, &mut crossings),
            Err(ReadError::NotAPoint { row: 1 })
        ));
    }

    #[test]
    fn corrupt_wkb_is_rejected() {
        let batch = batch_of(vec![Some(vec![0xff, 0x00, 0x01])]);

        assert!(matches!(
            read_one(&batch),
            Err(ReadError::Wkb { row: 0, .. })
        ));
    }

    #[test]
    fn a_missing_geometry_is_rejected() {
        let batch = batch_of(vec![None]);

        assert!(matches!(
            read_one(&batch),
            Err(ReadError::Null { row: 0, ref column }) if column == GEOMETRY
        ));
    }

    #[test]
    fn a_file_without_the_columns_is_rejected() {
        let batch =
            RecordBatch::try_from_iter(vec![("lon", Arc::new(Float64Array::from(vec![LON])) as _)])
                .unwrap();

        assert!(matches!(
            read_one(&batch),
            Err(ReadError::MissingColumn(ref column)) if column == RAIL_ID
        ));
    }

    /// A geometry column of numbers means the file is some other dataset, which is worth
    /// saying rather than reading a position out of whatever the bytes happen to be.
    #[test]
    fn a_column_holding_the_wrong_kind_of_value_is_rejected() {
        let batch = RecordBatch::try_from_iter(vec![
            (RAIL_ID, Arc::new(LargeStringArray::from(vec![RAIL])) as _),
            (WATER_ID, Arc::new(LargeStringArray::from(vec![WATER])) as _),
            (GEOMETRY, Arc::new(Float64Array::from(vec![LON])) as _),
        ])
        .unwrap();

        assert!(matches!(
            read_one(&batch),
            Err(ReadError::UnreadableColumn { ref column, .. }) if column == GEOMETRY
        ));
    }

    #[test]
    fn a_file_that_is_not_there_is_reported_with_its_path() {
        let missing = Path::new("data/water/nonexistent/crossing_reps.parquet");

        assert!(matches!(
            read(missing),
            Err(ReadError::Open { ref path, .. }) if path == &missing.display().to_string()
        ));
    }
}
