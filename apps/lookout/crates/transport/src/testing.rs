#![cfg(test)]

use arrow::array::RecordBatch;
use chrono::{DateTime, Utc};
use datafusion::execution::SendableRecordBatchStream;
use geo_types::{Geometry, polygon};
use medallion::{Country, Root};
use medallion_model::ExtractManifestRowV3;

use crate::overture::OvertureType;

pub(crate) fn square_from(corner: f64, size: f64) -> Geometry<f64> {
    Geometry::Polygon(polygon![
        (x: corner, y: corner),
        (x: corner + size, y: corner),
        (x: corner + size, y: corner + size),
        (x: corner, y: corner + size),
    ])
}

pub(crate) async fn record_extract(
    root: &Root,
    id: &str,
    country: Country,
    release: &str,
    taken_at: DateTime<Utc>,
    division_area: RecordBatch,
) {
    root.rows_of::<ExtractManifestRowV3>()
        .append_rows(
            taken_at,
            &[ExtractManifestRowV3 {
                extract_id: id.to_string(),
                extracted_at: taken_at,
                overture_release: release
                    .parse()
                    .expect("a test names a release Overture would"),
                country: country.code().to_string(),
                min_lon: 0.0,
                min_lat: 0.0,
                max_lon: 0.0,
                max_lat: 0.0,
            }],
        )
        .await
        .expect("record the extract");
    OvertureType::DIVISION_AREA
        .in_extract(root, id)
        .expect("the extract's division areas")
        .append_geo_stream(taken_at, stream_of(division_area).await)
        .await
        .expect("write the areas");
}

async fn stream_of(batch: RecordBatch) -> SendableRecordBatchStream {
    datafusion::prelude::SessionContext::new()
        .read_batch(batch)
        .expect("read the batch")
        .execute_stream()
        .await
        .expect("stream the batch")
}
