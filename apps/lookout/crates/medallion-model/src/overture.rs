use chrono::{DateTime, Utc};
use medallion::{DatasetSpec, Row, layers};
use serde::{Deserialize, Serialize};

pub const OVERTURE_EXTRACT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("overture_extract", "extract_id");

pub const EXTRACT_MANIFEST: DatasetSpec<layers::Bronze> =
    DatasetSpec::unpartitioned("extract_manifest");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractManifestRow {
    pub extract_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub extracted_at: DateTime<Utc>,
    pub release: String,
    pub country: String,
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl Row for ExtractManifestRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = EXTRACT_MANIFEST;
    const INSTANTS: &'static [&'static str] = &["extracted_at"];
}
