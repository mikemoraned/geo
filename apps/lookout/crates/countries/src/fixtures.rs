use std::sync::Arc;

use arrow::array::{RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::{DateTime, Utc};
use datafusion::execution::SendableRecordBatchStream;
use geo_types::{Geometry, polygon};
use medallion::{Country, GEOMETRY, Root};
use medallion_model::{DIVISION_ID, ExtractManifestRowV3, OvertureType};

pub fn square_from(corner: f64, size: f64) -> Geometry<f64> {
    Geometry::Polygon(polygon![
        (x: corner, y: corner),
        (x: corner + size, y: corner),
        (x: corner + size, y: corner + size),
        (x: corner, y: corner + size),
    ])
}

pub fn division_area(country: Country, areas: &[(&str, Geometry<f64>)]) -> RecordBatch {
    let geometries: Vec<Geometry<f64>> = areas.iter().map(|(_, area)| area.clone()).collect();
    let (geometry_field, geometry) = medallion::wkb_column(
        medallion::wkb_field(GEOMETRY).expect("a geometry field"),
        &geometries,
    )
    .expect("a geometry column");
    let schema = Schema::new(vec![
        Arc::new(Field::new(DIVISION_ID, DataType::Utf8, false)),
        Arc::new(Field::new("class", DataType::Utf8, false)),
        geometry_field,
    ]);
    let division = medallion_model::division_id(country).to_string();

    RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(StringArray::from(vec![division; areas.len()])),
            Arc::new(StringArray::from(
                areas.iter().map(|(class, _)| *class).collect::<Vec<_>>(),
            )),
            geometry,
        ],
    )
    .expect("a division_area batch")
}

pub async fn record_extract(
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
