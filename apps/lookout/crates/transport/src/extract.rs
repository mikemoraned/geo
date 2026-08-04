//! Taking a point-in-time extract of Overture into bronze.
//!
//! An extraction reads one release, restricted to one country, and writes the rows it
//! finds in Overture's own shape under its own id — plus one manifest row saying what was
//! taken, from which release, and over what window. The rows themselves gain a single
//! column, `extract_id`, joining them back to that manifest row.
//!
//! An extraction can also be taken again from what the manifest recorded, under the id it
//! was first taken under: a release is immutable, so the same release read again over the
//! same window is the same rows. That is how a store holding the manifest but not the rows
//! is filled, and it is a different operation from taking a new extract, which is why the
//! two are separate entry points rather than one with a flag.
//!
//! Every theme goes under one id in a single extraction, because they are read together:
//! the crossings derivation joins rail to water, and clips both against the country
//! boundary the window was measured from. Splitting them across extractions would let a
//! join span two releases.
//!
//! What is *not* done here is any shaping: no flattening of Overture's `bbox` struct, no
//! re-encoding of geometry, no dropping of columns a current query happens not to read.
//! Bronze keeps what arrived, and the derivations that read it shape it as they need.

use std::fmt::{self, Display};
use std::str::FromStr;

use arrow::array::{Array, Float64Array};
use chrono::{DateTime, Utc};
use geo_types::{Coord, Rect};
use medallion::{Country, PartitionValue, Query, Root};
use model::ExtractManifestRow;

use crate::overture::{Overture, OvertureError, OvertureType};

/// Rail `class`es left out of the extract: street `tram` lines aren't the transport we
/// care about, and excluding them here keeps their connectors out too.
const EXCLUDED_CLASSES: &[&str] = &["tram"];

/// The format an id generated from an instant takes: compact UTC, so ids sort
/// chronologically and carry no character a path or a column name would object to.
const ID_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// Every extraction a store has recorded, newest first.
const RECORDED: &str = "SELECT * FROM extract_manifest ORDER BY extracted_at DESC";

/// Identifies one extraction, and with it one immutable set of extracted rows.
///
/// An extract is keyed by an id rather than by a date because it is the unit of
/// immutability: re-extracting the same release over the same window is a new id, never a
/// replacement of an existing one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtractId(String);

impl ExtractId {
    /// An id for an extraction beginning at `at`.
    pub fn at(at: DateTime<Utc>) -> Self {
        Self(at.format(ID_FORMAT).to_string())
    }

    /// An existing id, rejecting anything that could not name a partition.
    pub fn new(id: impl Into<String>) -> Result<Self, medallion::PathError> {
        let id = id.into();
        PartitionValue::new(id.clone())?;
        Ok(Self(id))
    }
}

impl FromStr for ExtractId {
    type Err = medallion::PathError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for ExtractId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The extractions a store has recorded, newest first.
pub async fn recorded(root: &Root) -> Result<Vec<ExtractManifestRow>, ExtractError> {
    let query = Query::new(root.clone());
    query.register_by_name(model::EXTRACT_MANIFEST).await?;
    Ok(query.rows(RECORDED).await?)
}

/// What the newest extraction took.
pub async fn newest(root: &Root) -> Result<ExtractManifestRow, ExtractError> {
    recorded(root)
        .await?
        .into_iter()
        .next()
        .ok_or(ExtractError::NoExtract)
}

/// What the extraction with `id` took.
pub async fn recorded_as(root: &Root, id: &ExtractId) -> Result<ExtractManifestRow, ExtractError> {
    recorded(root)
        .await?
        .into_iter()
        .find(|recorded| recorded.extract_id == id.to_string())
        .ok_or_else(|| ExtractError::NoSuchExtract { id: id.clone() })
}

/// Failure taking an extract.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("querying Overture: {0}")]
    Overture(#[from] OvertureError),
    #[error("reading the manifest: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("partitioning the extract: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the extract: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("writing the manifest: {0}")]
    Append(#[from] medallion::AppendError),
    #[error("{country} has no boundary in release {release}, so its window is unknown")]
    NoCountryBoundary { country: Country, release: String },
    #[error("no extract has been recorded, so there is none to take again")]
    NoExtract,
    #[error("no extract {id} in the manifest")]
    NoSuchExtract { id: ExtractId },
    #[error("extract {id} recorded country {country}, which the store has no zone for")]
    UnknownCountry {
        id: ExtractId,
        country: String,
        #[source]
        source: medallion::UnknownCountry,
    },
    #[error(
        "extract {id} already holds rows in the store; an extract is immutable, so taking \
         it again would double its rows rather than replace them"
    )]
    AlreadyPresent { id: ExtractId },
    #[error(
        "extract {id} was taken from release {recorded}, but this reads {opened}; a \
         re-fetch has to read the release the extract was taken from"
    )]
    WrongRelease {
        id: ExtractId,
        recorded: String,
        opened: String,
    },
}

/// What one extraction produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Extraction {
    pub id: ExtractId,
    pub window: Rect<f64>,
    pub rows: Vec<(OvertureType, usize)>,
}

/// Takes extracts of one release into one store.
pub struct Extractor<'a> {
    overture: &'a Overture,
    root: &'a Root,
}

impl<'a> Extractor<'a> {
    pub fn new(overture: &'a Overture, root: &'a Root) -> Self {
        Self { overture, root }
    }

    /// Extract everything the crossings pipeline reads, for `country`, under a single id.
    ///
    /// The window is the country's own bounding box, taken from the release's boundary for
    /// it, so it is a property of the release rather than of whatever has been observed so
    /// far. Narrowing to an area of interest belongs to the derivations reading this.
    pub async fn extract(
        &self,
        id: ExtractId,
        country: Country,
        at: DateTime<Utc>,
    ) -> Result<Extraction, ExtractError> {
        self.register_themes().await?;
        let window = self.country_window(country).await?;
        let rows = self.write_types(&id, country, &window, at).await?;
        self.write_manifest(&id, country, at, &window).await?;

        Ok(Extraction { id, window, rows })
    }

    /// Take a recorded extraction again: the same release, country and window, written back
    /// under the id it was taken under.
    ///
    /// This is how a store that has the manifest but not the rows is filled — the rows are
    /// re-derivable from what the manifest records, since a release never changes. No
    /// manifest row is written: the one being read is already the record of this extract,
    /// and it names the instant the extraction was originally taken.
    ///
    /// An extract that already holds rows is not re-fetched. Extracts are immutable, so
    /// there is nothing here that could replace them, and writing again would append a
    /// second copy of every row.
    pub async fn backfill(
        &self,
        recorded: &ExtractManifestRow,
        at: DateTime<Utc>,
    ) -> Result<Extraction, ExtractError> {
        let id = ExtractId::new(recorded.extract_id.clone())?;
        let country = recorded
            .country
            .parse()
            .map_err(|source| ExtractError::UnknownCountry {
                id: id.clone(),
                country: recorded.country.clone(),
                source,
            })?;
        let window = window_of(recorded);

        let opened = self.overture.release().id();
        if opened != recorded.release {
            return Err(ExtractError::WrongRelease {
                id,
                recorded: recorded.release.clone(),
                opened: opened.to_string(),
            });
        }
        if self
            .root
            .dataset(model::OVERTURE_EXTRACT)
            .for_id(&id)?
            .holds_files()
        {
            return Err(ExtractError::AlreadyPresent { id });
        }

        self.register_themes().await?;
        let rows = self.write_types(&id, country, &window, at).await?;

        Ok(Extraction { id, window, rows })
    }

    /// Register every Overture type an extraction reads.
    async fn register_themes(&self) -> Result<(), ExtractError> {
        for (overture_type, table) in [
            (OvertureType::DIVISION_AREA, "division_area"),
            (OvertureType::DIVISION, "division"),
            (OvertureType::SEGMENT, "segments"),
            (OvertureType::CONNECTOR, "connectors"),
            (OvertureType::WATER, "water"),
        ] {
            self.overture.register(overture_type, table).await?;
        }
        Ok(())
    }

    /// Write every type the crossings pipeline reads, for one country over one window,
    /// under a single id.
    async fn write_types(
        &self,
        id: &ExtractId,
        country: Country,
        window: &Rect<f64>,
        at: DateTime<Utc>,
    ) -> Result<Vec<(OvertureType, usize)>, ExtractError> {
        let in_window = bbox_overlaps(window);
        let of_country = format!("country = '{}'", country.code());
        let rail = format!(
            "subtype = 'rail' AND {class} AND {in_window}",
            class = class_filter(),
        );

        let mut rows = Vec::new();
        for (overture_type, table, predicate) in [
            (
                OvertureType::DIVISION_AREA,
                "division_area",
                of_country.clone(),
            ),
            (OvertureType::DIVISION, "division", of_country),
            (OvertureType::SEGMENT, "segments", rail.clone()),
            (OvertureType::WATER, "water", in_window.clone()),
            (
                OvertureType::CONNECTOR,
                "connectors",
                referenced_connectors(&in_window, &rail),
            ),
        ] {
            let written = self.write(id, at, overture_type, table, &predicate).await?;
            tracing::info!(
                theme = overture_type.theme,
                r#type = overture_type.name,
                rows = written,
                "extracted",
            );
            rows.push((overture_type, written));
        }

        Ok(rows)
    }

    /// Extract one type's rows matching `predicate` into the extract, verbatim but for the
    /// `extract_id` joining them to the manifest.
    async fn write(
        &self,
        id: &ExtractId,
        at: DateTime<Utc>,
        overture_type: OvertureType,
        table: &str,
        predicate: &str,
    ) -> Result<usize, ExtractError> {
        let stream = self
            .overture
            .stream(&format!(
                "SELECT *, '{id}' AS extract_id FROM {table} WHERE {predicate}"
            ))
            .await?;
        Ok(self
            .root
            .dataset(model::OVERTURE_EXTRACT)
            .for_id(id)?
            .partition("theme", overture_type.theme)?
            .partition("type", overture_type.name)?
            .append_geo_stream(at, stream)
            .await?
            .rows)
    }

    /// The bounding box of `country`'s own boundary in this release.
    async fn country_window(&self, country: Country) -> Result<Rect<f64>, ExtractError> {
        let batches = self
            .overture
            .sql(&format!(
                "SELECT MIN(bbox.xmin) AS min_lon, MIN(bbox.ymin) AS min_lat,
                        MAX(bbox.xmax) AS max_lon, MAX(bbox.ymax) AS max_lat
                 FROM division_area
                 WHERE subtype = 'country' AND country = '{}'",
                country.code()
            ))
            .await?;
        let missing = || ExtractError::NoCountryBoundary {
            country,
            release: self.overture.release().id().to_string(),
        };

        let batch = batches.first().ok_or_else(missing)?;
        let corner = |name: &str| -> Option<f64> {
            let column = batch.column_by_name(name)?;
            let values = column.as_any().downcast_ref::<Float64Array>()?;
            (!values.is_empty() && values.is_valid(0)).then(|| values.value(0))
        };

        Ok(Rect::new(
            Coord {
                x: corner("min_lon").ok_or_else(missing)?,
                y: corner("min_lat").ok_or_else(missing)?,
            },
            Coord {
                x: corner("max_lon").ok_or_else(missing)?,
                y: corner("max_lat").ok_or_else(missing)?,
            },
        ))
    }

    /// Record what this extraction took. Written last: a manifest row is the claim that an
    /// extract is complete, so it must not exist before the rows it describes.
    async fn write_manifest(
        &self,
        id: &ExtractId,
        country: Country,
        at: DateTime<Utc>,
        window: &Rect<f64>,
    ) -> Result<(), ExtractError> {
        let row = ExtractManifestRow {
            extract_id: id.to_string(),
            extracted_at: at,
            release: self.overture.release().id().to_string(),
            country: country.code().to_string(),
            min_lon: window.min().x,
            min_lat: window.min().y,
            max_lon: window.max().x,
            max_lat: window.max().y,
        };
        self.root
            .rows_of::<ExtractManifestRow>()
            .append_rows(at, &[row])
            .await?;
        Ok(())
    }
}

/// The window a manifest row records, as the rectangle the predicates are built from.
fn window_of(recorded: &ExtractManifestRow) -> Rect<f64> {
    Rect::new(
        Coord {
            x: recorded.min_lon,
            y: recorded.min_lat,
        },
        Coord {
            x: recorded.max_lon,
            y: recorded.max_lat,
        },
    )
}

/// A predicate keeping rows whose Overture `bbox` overlaps `window`. Overture stores the
/// envelope on every row, so this prunes row groups before any geometry is decoded.
fn bbox_overlaps(window: &Rect<f64>) -> String {
    format!(
        "bbox.xmin <= {max_lon} AND bbox.xmax >= {min_lon}
         AND bbox.ymin <= {max_lat} AND bbox.ymax >= {min_lat}",
        min_lon = window.min().x,
        min_lat = window.min().y,
        max_lon = window.max().x,
        max_lat = window.max().y,
    )
}

/// A predicate keeping connectors in the window that a kept rail segment refers to.
///
/// Restricting to referenced connectors rather than to every connector in the window is
/// what keeps this to rail: the window holds every road junction in the country too.
fn referenced_connectors(in_window: &str, rail: &str) -> String {
    format!(
        "{in_window}
         AND id IN (
           SELECT DISTINCT elem['connector_id']
           FROM (SELECT UNNEST(s.connectors) AS elem FROM segments AS s WHERE {rail}) AS refs
         )"
    )
}

/// A SQL predicate excluding [`EXCLUDED_CLASSES`], keeping null-class rows (`coalesce` maps
/// a null class to `''`, which is never an excluded class). `TRUE` when nothing is excluded,
/// so it composes into an `AND` chain unconditionally.
fn class_filter() -> String {
    if EXCLUDED_CLASSES.is_empty() {
        return "TRUE".to_string();
    }
    let excluded = EXCLUDED_CLASSES
        .iter()
        .map(|class| format!("'{class}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("coalesce(class, '') NOT IN ({excluded})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overture::Release;
    use chrono::TimeZone;

    fn window() -> Rect<f64> {
        Rect::new(Coord { x: 5.8, y: 47.2 }, Coord { x: 15.1, y: 55.1 })
    }

    fn manifest_row(id: &str, hour: u32, release: &str) -> ExtractManifestRow {
        ExtractManifestRow {
            extract_id: id.to_string(),
            extracted_at: Utc.with_ymd_and_hms(2026, 7, 27, hour, 0, 0).unwrap(),
            release: release.to_string(),
            country: Country::Germany.code().to_string(),
            min_lon: window().min().x,
            min_lat: window().min().y,
            max_lon: window().max().x,
            max_lat: window().max().y,
        }
    }

    async fn store_recording(rows: &[ExtractManifestRow]) -> (tempfile::TempDir, Root) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        for row in rows {
            root.rows_of::<ExtractManifestRow>()
                .append_rows(row.extracted_at, std::slice::from_ref(row))
                .await
                .expect("record the extraction");
        }
        (tmp, root)
    }

    /// A recorded extraction comes back as it was written, which is what makes an extract
    /// re-fetchable from the manifest alone.
    #[tokio::test]
    async fn the_newest_recorded_extraction_is_the_last_one_taken() {
        let (_tmp, root) = store_recording(&[
            manifest_row("20260727T090000Z", 9, "2026-05-21.0"),
            manifest_row("20260727T193628Z", 19, "2026-06-17.0"),
        ])
        .await;

        let newest = newest(&root).await.expect("the newest extraction");

        assert_eq!(newest, manifest_row("20260727T193628Z", 19, "2026-06-17.0"));
    }

    #[tokio::test]
    async fn an_extraction_can_be_read_back_by_the_id_it_was_taken_under() {
        let (_tmp, root) = store_recording(&[
            manifest_row("20260727T090000Z", 9, "2026-05-21.0"),
            manifest_row("20260727T193628Z", 19, "2026-06-17.0"),
        ])
        .await;
        let wanted = "20260727T090000Z".parse().expect("a valid id");

        let recorded = recorded_as(&root, &wanted).await.expect("the extraction");

        assert_eq!(recorded.release, "2026-05-21.0");
    }

    #[tokio::test]
    async fn an_id_no_extraction_was_taken_under_is_not_found() {
        let (_tmp, root) =
            store_recording(&[manifest_row("20260727T193628Z", 19, "2026-06-17.0")]).await;
        let wanted = "20260101T000000Z".parse().expect("a valid id");

        let err = recorded_as(&root, &wanted).await;

        assert!(matches!(err, Err(ExtractError::NoSuchExtract { .. })));
    }

    /// A store nothing has been extracted into has no manifest to read, and so nothing to
    /// take again.
    #[tokio::test]
    async fn a_store_with_no_manifest_has_no_extraction_to_take_again() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let err = newest(&Root::new(tmp.path())).await;

        assert!(matches!(
            err,
            Err(ExtractError::Query(
                medallion::QueryError::NoSuchDataset { .. }
            ))
        ));
    }

    /// Re-fetching an extract whose rows are already there would append a second copy of
    /// every one of them: bronze has no replace, so this is refused rather than doubled.
    #[tokio::test]
    async fn an_extract_already_in_the_store_is_not_taken_again() {
        let recorded = manifest_row("20260727T193628Z", 19, "2026-06-17.0");
        let (_tmp, root) = store_recording(std::slice::from_ref(&recorded)).await;
        let extract = root
            .dataset(model::OVERTURE_EXTRACT)
            .for_id(&recorded.extract_id)
            .unwrap()
            .partition("theme", "base")
            .unwrap();
        std::fs::create_dir_all(extract.dir()).unwrap();
        std::fs::write(extract.dir().join("already.parquet"), b"rows").unwrap();
        let overture = Overture::open(Release::published(&recorded.release));

        let err = Extractor::new(&overture, &root)
            .backfill(&recorded, Utc::now())
            .await;

        assert!(matches!(err, Err(ExtractError::AlreadyPresent { .. })));
    }

    /// An extract's id names rows taken from one release, so filling it from a different
    /// release would put rows under an id claiming provenance they do not have.
    #[tokio::test]
    async fn an_extract_is_not_filled_from_a_release_it_was_not_taken_from() {
        let recorded = manifest_row("20260727T193628Z", 19, "2026-06-17.0");
        let (_tmp, root) = store_recording(std::slice::from_ref(&recorded)).await;
        let overture = Overture::open(Release::published("2026-05-21.0"));

        let err = Extractor::new(&overture, &root)
            .backfill(&recorded, Utc::now())
            .await;

        assert!(matches!(err, Err(ExtractError::WrongRelease { .. })));
    }

    #[test]
    fn an_id_from_an_instant_is_compact_utc() {
        let at = Utc.with_ymd_and_hms(2026, 7, 27, 20, 45, 30).unwrap();

        assert_eq!(ExtractId::at(at).to_string(), "20260727T204530Z");
    }

    /// Ids sort chronologically, so the latest extract of a release is the last one.
    #[test]
    fn ids_from_instants_sort_in_the_order_they_were_taken() {
        let earlier = ExtractId::at(Utc.with_ymd_and_hms(2026, 7, 27, 20, 45, 30).unwrap());
        let later = ExtractId::at(Utc.with_ymd_and_hms(2026, 7, 27, 20, 45, 31).unwrap());

        assert!(earlier.to_string() < later.to_string());
    }

    #[test]
    fn an_id_that_could_not_name_a_partition_is_rejected() {
        assert!(ExtractId::new("2026/07/27").is_err());
        assert!(ExtractId::new("").is_err());
        assert_eq!(
            "20260727T204530Z".parse::<ExtractId>().unwrap().to_string(),
            "20260727T204530Z"
        );
    }

    /// Overlap, not containment: a river running off the edge of the window still crosses
    /// rail inside it, so a row is kept when its envelope touches the window at all.
    #[test]
    fn the_window_predicate_keeps_rows_whose_envelope_overlaps_it() {
        let predicate = bbox_overlaps(&window());

        assert!(predicate.contains("bbox.xmin <= 15.1"));
        assert!(predicate.contains("bbox.xmax >= 5.8"));
        assert!(predicate.contains("bbox.ymin <= 55.1"));
        assert!(predicate.contains("bbox.ymax >= 47.2"));
    }

    /// Connectors are restricted to those a kept rail segment names, so the country's road
    /// junctions don't come along with them.
    #[test]
    fn connectors_are_restricted_to_those_rail_segments_refer_to() {
        let predicate = referenced_connectors(&bbox_overlaps(&window()), "subtype = 'rail'");

        assert!(predicate.contains("UNNEST(s.connectors)"));
        assert!(predicate.contains("subtype = 'rail'"));
        assert!(predicate.contains("bbox.xmin <= 15.1"));
    }

    #[test]
    fn the_class_filter_excludes_configured_classes_keeping_nulls() {
        assert!(
            !EXCLUDED_CLASSES.is_empty(),
            "expects at least one exclusion"
        );
        let filter = class_filter();

        assert!(filter.starts_with("coalesce(class, '') NOT IN ("));
        for class in EXCLUDED_CLASSES {
            assert!(filter.contains(&format!("'{class}'")));
        }
    }
}
