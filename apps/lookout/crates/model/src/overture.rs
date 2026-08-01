//! The Overture extracts and the manifest recording what each extraction took.

use chrono::{DateTime, Utc};
use medallion::{DatasetSpec, Row, layers};
use serde::{Deserialize, Serialize};

/// Overture Maps rows as extracted, in Overture's own shape and directory layout below
/// the id of the extraction that fetched them.
///
/// The rows have no row type here: they keep whatever columns the release's own schema
/// gives them, plus the `extract_id` joining them to [`ExtractManifestRow`].
pub const OVERTURE_EXTRACT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("overture_extract", "extract_id");

/// One row per extraction: what it fetched, from which release, and when. The provenance
/// [`OVERTURE_EXTRACT`]'s rows carry only the id of.
pub const EXTRACT_MANIFEST: DatasetSpec<layers::Bronze> =
    DatasetSpec::unpartitioned("extract_manifest");

/// One row of the manifest: what an extraction took, and from where.
///
/// The window is stored as four numbers rather than as a geometry because it is provenance
/// — the answer to "what was this restricted to" — and a reader checking whether an extract
/// covers an area of interest compares numbers.
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
