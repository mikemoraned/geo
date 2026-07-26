//! Writing silver GeoParquet: WKB geometry, simple features, CRS in the file metadata as
//! PROJJSON.
//!
//! The metadata is produced by the `geoparquet` encoder rather than assembled here, so the
//! files conform to the spec version that crate implements.

use std::path::Path;
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::{DataType, Field};
use geoarrow_schema::{Crs, Metadata, WkbType};
use geoparquet::writer::{GeoParquetRecordBatchEncoder, GeoParquetWriterOptions};
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use parquet::arrow::async_writer::ParquetObjectWriter;
use parquet::arrow::AsyncArrowWriter;

use crate::write::WriteError;

/// The global CRS every silver geometry is stored in, as PROJJSON — the encoding
/// GeoParquet requires. Generated from PROJ by `just crs-definitions`.
const CRS84_PROJJSON: &str = include_str!("crs84.projjson.json");

/// Failure describing a geometry column.
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
}

/// A WKB geometry field in [CRS 84](https://www.opengis.net/def/crs/OGC/1.3/CRS84), for
/// use in a silver schema.
///
/// The returned [`Field`] carries the GeoArrow extension metadata the writer reads to
/// produce GeoParquet's `geo` file metadata, so callers build their schema with it rather
/// than declaring a plain binary column.
pub fn wkb_field(name: &str) -> Result<Field, GeoError> {
    let crs = Crs::from_projjson(serde_json::from_str(CRS84_PROJJSON)?);
    let metadata = Arc::new(Metadata::new(crs, None));
    Ok(Field::new(name, DataType::Binary, true).with_extension_type(WkbType::new(metadata)))
}

/// Write `batches` to `path` as a single GeoParquet file.
///
/// The batches' schema must carry GeoArrow metadata on its geometry columns — see
/// [`wkb_field`]. Like [`crate::write_batches`], the file appears at `path` only once
/// fully written.
pub async fn write_geo_batches(path: &Path, batches: &[RecordBatch]) -> Result<(), GeoError> {
    let Some(first) = batches.first() else {
        return Err(WriteError::Empty.into());
    };

    let options = GeoParquetWriterOptions::default();
    let mut encoder = GeoParquetRecordBatchEncoder::try_new(first.schema().as_ref(), &options)?;

    let store = LocalFileSystem::new();
    let location = ObjectPath::from_absolute_path(path).map_err(|source| WriteError::Path {
        path: path.display().to_string(),
        source,
    })?;
    let object_writer = ParquetObjectWriter::new(Arc::new(store), location);
    let mut writer = AsyncArrowWriter::try_new(object_writer, encoder.target_schema(), None)?;

    for batch in batches {
        writer.write(&encoder.encode_record_batch(batch)?).await?;
    }
    writer.append_key_value_metadata(encoder.into_keyvalue()?);
    writer.close().await?;
    Ok(())
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
