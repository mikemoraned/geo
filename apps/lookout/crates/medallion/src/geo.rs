use std::path::Path;
use std::sync::Arc;

use datafusion::execution::SendableRecordBatchStream;
use futures::StreamExt;

use crate::country::Country;
use crate::rows::{Row, RowError, fields};
use crate::write::WriteError;
use arrow::array::{Array, ArrayRef, BinaryArray, RecordBatch};
use arrow::datatypes::{DataType, Field, FieldRef, Schema};
use geoarrow_schema::{Crs, Metadata, WkbType};
use geoparquet::writer::{GeoParquetRecordBatchEncoder, GeoParquetWriterOptions};

const CRS84_PROJJSON: &str = include_str!("crs84.projjson.json");

const CRS84_EPSG: u16 = 4326;

pub const GEOMETRY: &str = "geometry";

pub const PROJECTED_GEOMETRY: &str = "geometry_projected";

#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    #[error("the bundled CRS84 PROJJSON is not valid json: {0}")]
    Crs(#[from] serde_json::Error),
    #[error("geoarrow error: {0}")]
    GeoArrow(#[from] geoarrow_schema::error::GeoArrowError),
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("projecting geometry: {0}")]
    Projection(#[from] proj4rs::errors::Error),
    #[error("wkb error: {0}")]
    Wkb(#[from] wkb::error::WkbError),
    #[error("reading the column: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("no geometry column named {0}")]
    NoSuchColumn(String),
    #[error("reading the batches to write: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
    #[error(transparent)]
    Row(#[from] RowError),
    #[error("building the columns: {0}")]
    Encode(#[from] serde_arrow::Error),
}

pub fn wkb_field(name: &str) -> Result<Field, GeoError> {
    let crs = Crs::from_projjson(serde_json::from_str(CRS84_PROJJSON)?);
    let metadata = Arc::new(Metadata::new(crs, None));
    Ok(Field::new(name, DataType::Binary, true).with_extension_type(WkbType::new(metadata)))
}

pub fn projected_wkb_field(name: &str, country: Country) -> Result<Field, GeoError> {
    let crs = Crs::from_projjson(serde_json::from_str(country.projected_projjson())?);
    let metadata = Arc::new(Metadata::new(crs, None));
    Ok(Field::new(name, DataType::Binary, true).with_extension_type(WkbType::new(metadata)))
}

pub struct Projector {
    from: proj4rs::Proj,
    to: proj4rs::Proj,
}

impl Projector {
    pub fn for_country(country: Country) -> Result<Self, GeoError> {
        Ok(Self {
            from: proj4rs::Proj::from_epsg_code(CRS84_EPSG)?,
            to: proj4rs::Proj::from_epsg_code(country.projected_epsg())?,
        })
    }

    pub fn project<G>(&self, geometry: &G) -> Result<G, GeoError>
    where
        G: geo::MapCoords<f64, f64, Output = G>,
    {
        geometry.try_map_coords(|coord| {
            // proj4rs takes radians for a geographic system, whatever the source states.
            let mut point = (coord.x.to_radians(), coord.y.to_radians(), 0.0);
            proj4rs::transform::transform(&self.from, &self.to, &mut point)?;
            Ok::<_, GeoError>(geo_types::coord! { x: point.0, y: point.1 })
        })
    }
}

pub fn wkb_column<G>(field: Field, geometries: &[G]) -> Result<(FieldRef, ArrayRef), GeoError>
where
    G: geo_traits::GeometryTrait<T = f64>,
{
    let encoded = geometries
        .iter()
        .map(|geometry| {
            let mut buf = Vec::new();
            wkb::writer::write_geometry(&mut buf, geometry, &wkb::writer::WriteOptions::default())?;
            Ok(buf)
        })
        .collect::<Result<Vec<_>, wkb::error::WkbError>>()?;
    Ok((
        Arc::new(field),
        Arc::new(BinaryArray::from_iter_values(encoded)),
    ))
}

pub fn geo_batch<T, G>(rows: &[T], geometry: &[(Field, &[G])]) -> Result<RecordBatch, GeoError>
where
    T: Row,
    G: geo_traits::GeometryTrait<T = f64>,
{
    let mut columns = fields::<T>()?;
    let mut arrays = serde_arrow::to_arrow(&columns, rows)?;

    for (field, geometries) in geometry {
        let (field, array) = wkb_column(field.clone(), geometries)?;
        columns.push(field);
        arrays.push(array);
    }

    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(columns)),
        arrays,
    )?)
}

pub fn geometries(
    batch: &RecordBatch,
    column: &str,
) -> Result<Vec<geo_types::Geometry<f64>>, GeoError> {
    let array = batch
        .column_by_name(column)
        .ok_or_else(|| GeoError::NoSuchColumn(column.to_string()))?;
    let binary = arrow::compute::cast(array, &DataType::Binary)?;
    let binary = binary
        .as_any()
        .downcast_ref::<BinaryArray>()
        .ok_or_else(|| GeoError::NoSuchColumn(column.to_string()))?;

    (0..binary.len())
        .map(|i| {
            let geometry = wkb::reader::read_wkb(binary.value(i))?;
            Ok(geo_traits::to_geo::ToGeoGeometry::to_geometry(&geometry))
        })
        .collect()
}

pub(crate) async fn write_geo_batches(
    path: &Path,
    batches: &[RecordBatch],
) -> Result<(), GeoError> {
    let Some(first) = batches.first() else {
        return Err(WriteError::Empty.into());
    };

    let options = GeoParquetWriterOptions::default();
    let mut encoder = GeoParquetRecordBatchEncoder::try_new(first.schema().as_ref(), &options)?;
    let mut writer = crate::write::writer_at(path, encoder.target_schema())?;

    for batch in batches {
        writer.write(&encoder.encode_record_batch(batch)?).await?;
    }
    writer.append_key_value_metadata(encoder.into_keyvalue()?);
    writer.close().await?;
    Ok(())
}

pub(crate) async fn write_geo_stream(
    path: &Path,
    mut batches: SendableRecordBatchStream,
) -> Result<usize, GeoError> {
    let options = GeoParquetWriterOptions::default();
    let mut encoder = GeoParquetRecordBatchEncoder::try_new(batches.schema().as_ref(), &options)?;
    let mut writer = crate::write::writer_at(path, encoder.target_schema())?;

    let mut rows = 0;
    while let Some(batch) = batches.next().await {
        let batch = batch?;
        rows += batch.num_rows();
        writer.write(&encoder.encode_record_batch(&batch)?).await?;
    }
    writer.append_key_value_metadata(encoder.into_keyvalue()?);
    writer.close().await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_crs_is_projjson_for_crs84() {
        let crs: serde_json::Value = serde_json::from_str(CRS84_PROJJSON).unwrap();

        assert_eq!(crs["id"]["authority"], "OGC");
        assert_eq!(crs["id"]["code"], "CRS84");
        assert_eq!(crs["type"], "GeographicCRS");
    }

    #[test]
    fn a_projected_field_carries_the_projected_crs() {
        let field = projected_wkb_field("geometry_utm", Country::Germany).unwrap();

        let extension: serde_json::Value =
            serde_json::from_str(field.metadata().get("ARROW:extension:metadata").unwrap())
                .unwrap();
        assert_eq!(extension["crs"]["id"]["code"], 25832);
    }

    #[test]
    fn projecting_lat_lon_yields_metres_in_the_german_zone() {
        let projector = Projector::for_country(Country::Germany).unwrap();

        let projected = projector
            .project(&geo_types::Point::new(13.404954, 52.520008))
            .unwrap();

        assert!(
            (projected.x() - 798_809.63).abs() < 0.01
                && (projected.y() - 5_828_000.60).abs() < 0.01,
            "unexpected projection: {projected:?}"
        );
    }

    #[test]
    fn projecting_a_line_string_projects_every_coordinate() {
        let projector = Projector::for_country(Country::Germany).unwrap();
        let line = geo_types::LineString::from(vec![(13.404954, 52.520008), (8.682127, 50.110924)]);

        let projected = projector.project(&line).unwrap();

        let coords: Vec<_> = projected.coords().collect();
        assert!((coords[0].x - 798_809.63).abs() < 0.01);
        assert!((coords[1].x - 477_271.45).abs() < 0.01);
        assert!((coords[1].y - 5_551_012.24).abs() < 0.01);
    }

    #[test]
    fn a_wkb_field_carries_the_geoarrow_extension_and_crs() {
        let field = wkb_field("geometry").unwrap();

        assert_eq!(
            field.metadata().get("ARROW:extension:name").unwrap(),
            "geoarrow.wkb"
        );
        let extension: serde_json::Value =
            serde_json::from_str(field.metadata().get("ARROW:extension:metadata").unwrap())
                .unwrap();
        assert_eq!(extension["crs"]["id"]["code"], "CRS84");
    }
}
