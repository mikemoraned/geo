//! The multi-engine invariant from `docs/medallion.md`: **any file in silver must be
//! readable by every engine in use, with no engine-specific handling**.
//!
//! One silver GeoParquet file is written from Rust, then read back by each engine — DuckDB,
//! SedonaDB, and georust — and each must yield identical geometry and the same CRS. This is
//! a standing check, not a one-off: it runs in the default test profile, so silver drifting
//! engine-specific fails the build.

use std::sync::Arc;

use arrow::array::{Array, BinaryArray, Int64Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use geo_types::{Geometry, Point};
use medallion::{wkb_field, write_geo_batches, Layer, Root};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

/// Points chosen inside Germany, so the fixture is the shape silver actually holds.
const POINTS: [(i64, f64, f64); 3] = [
    (1, 13.404954, 52.520008),
    (2, 11.581981, 48.135125),
    (3, 8.682127, 50.110924),
];

fn expected_geometries() -> Vec<Geometry<f64>> {
    POINTS
        .iter()
        .map(|(_, lon, lat)| Geometry::Point(Point::new(*lon, *lat)))
        .collect()
}

/// A silver-shaped batch: an id column and a WKB geometry column carrying its CRS.
fn silver_batch() -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        wkb_field("geometry").unwrap(),
    ]));
    let wkb: Vec<Vec<u8>> = expected_geometries()
        .iter()
        .map(|geometry| {
            let mut buf = Vec::new();
            wkb::writer::write_geometry(&mut buf, geometry, &wkb::writer::WriteOptions::default())
                .unwrap();
            buf
        })
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(
                POINTS.iter().map(|(id, ..)| *id).collect::<Vec<_>>(),
            )),
            Arc::new(BinaryArray::from_iter_values(wkb)),
        ],
    )
    .unwrap()
}

fn decode(wkb_bytes: &[u8]) -> Geometry<f64> {
    let geometry = wkb::reader::read_wkb(wkb_bytes).unwrap();
    geo_traits::to_geo::ToGeoGeometry::to_geometry(&geometry)
}

/// Decode a WKB column, whichever binary layout the engine handed back (`Binary`,
/// `LargeBinary`, `BinaryView`), by casting it to `Binary` first.
fn decode_column(batch: &RecordBatch) -> Vec<Geometry<f64>> {
    let column =
        arrow::compute::cast(batch.column_by_name("geometry").unwrap(), &DataType::Binary).unwrap();
    let column = column.as_any().downcast_ref::<BinaryArray>().unwrap();
    (0..column.len()).map(|i| decode(column.value(i))).collect()
}

/// The `id` of the CRS recorded in a PROJJSON document, as `authority:code`.
fn crs_id(projjson: &serde_json::Value) -> String {
    format!(
        "{}:{}",
        projjson["id"]["authority"].as_str().unwrap(),
        projjson["id"]["code"].as_str().unwrap()
    )
}

/// Write the fixture into a silver partition of a throwaway store.
async fn write_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let path = Root::new(dir)
        .dataset(Layer::Silver, "water_crossing")
        .partition("country", "DE")
        .unwrap()
        .file("part-0");
    write_geo_batches(&path, &[silver_batch()]).await.unwrap();
    path
}

/// georust: the parquet reader plus `wkb` decoding into `geo-types`, with the GeoParquet
/// metadata read straight off the file's key-value metadata.
fn read_with_georust(path: &std::path::Path) -> (Vec<Geometry<f64>>, String) {
    let file = std::fs::File::open(path).unwrap();
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();

    let geo = builder
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .unwrap()
        .iter()
        .find(|kv| kv.key == "geo")
        .and_then(|kv| kv.value.as_ref())
        .expect("no GeoParquet `geo` metadata");
    let geo: serde_json::Value = serde_json::from_str(geo).unwrap();
    let crs = crs_id(&geo["columns"]["geometry"]["crs"]);

    let geometries = builder
        .build()
        .unwrap()
        .map(|batch| batch.unwrap())
        .flat_map(|batch| decode_column(&batch))
        .collect();

    (geometries, crs)
}

/// DuckDB, in-process: `read_parquet` for the rows, `parquet_kv_metadata` for the CRS.
fn read_with_duckdb(path: &std::path::Path) -> (Vec<Geometry<f64>>, String) {
    let conn = duckdb::Connection::open_in_memory().unwrap();
    let path = path.display().to_string();

    let mut statement = conn
        .prepare("SELECT geometry FROM read_parquet(?) ORDER BY id")
        .unwrap();
    let geometries = statement
        .query_map([&path], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|wkb_bytes| decode(&wkb_bytes.unwrap()))
        .collect();

    let mut statement = conn
        .prepare("SELECT value FROM parquet_kv_metadata(?) WHERE key = 'geo'")
        .unwrap();
    let geo: Vec<u8> = statement
        .query_row([&path], |row| row.get(0))
        .expect("no GeoParquet `geo` metadata");
    let geo: serde_json::Value = serde_json::from_slice(&geo).unwrap();

    (geometries, crs_id(&geo["columns"]["geometry"]["crs"]))
}

/// SedonaDB: reads the file as GeoParquet, so geometry comes back as a geometry type and
/// the CRS off the schema rather than from raw file metadata.
async fn read_with_sedona(path: &std::path::Path) -> (Vec<Geometry<f64>>, String) {
    let ctx = sedona::context::SedonaContext::new();
    let options = sedona_geoparquet::provider::GeoParquetReadOptions::default();
    let df = ctx
        .read_parquet(path.display().to_string(), options)
        .await
        .unwrap();
    ctx.ctx.register_table("silver", df.into_view()).unwrap();

    let batches = ctx
        .sql("SELECT ST_AsBinary(geometry) AS geometry FROM silver ORDER BY id")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    let geometries = batches.iter().flat_map(decode_column).collect();

    let schema = ctx.ctx.table("silver").await.unwrap().schema().clone();
    let field = schema.field_with_name(None, "geometry").unwrap();
    let extension = field
        .metadata()
        .get("ARROW:extension:metadata")
        .expect("no geoarrow extension metadata on the geometry field");
    let extension: serde_json::Value = serde_json::from_str(extension).unwrap();

    (geometries, crs_id(&extension["crs"]))
}

#[tokio::test]
async fn every_engine_reads_the_same_geometry_and_crs_from_one_silver_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_fixture(tmp.path()).await;

    let (georust_geometries, georust_crs) = read_with_georust(&path);
    let (duckdb_geometries, duckdb_crs) = read_with_duckdb(&path);
    let (sedona_geometries, sedona_crs) = read_with_sedona(&path).await;

    assert_eq!(
        georust_geometries,
        expected_geometries(),
        "georust geometry"
    );
    assert_eq!(duckdb_geometries, expected_geometries(), "duckdb geometry");
    assert_eq!(sedona_geometries, expected_geometries(), "sedona geometry");

    assert_eq!(georust_crs, "OGC:CRS84", "georust crs");
    assert_eq!(duckdb_crs, "OGC:CRS84", "duckdb crs");
    assert_eq!(sedona_crs, "OGC:CRS84", "sedona crs");
}
