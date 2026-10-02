use std::fmt::{self, Display};
use std::str::FromStr;

use arrow::array::{Array, Float64Array};
use chrono::{DateTime, Utc};
use geo_types::{Coord, Rect};
use medallion::{Country, PartitionValue, Query, Root};
use medallion_model::{DIVISION_ID, ExtractManifestRow};

use crate::overture::{Overture, OvertureError, OvertureType};

const EXCLUDED_CLASSES: &[&str] = &["tram"];

const COMPACT_UTC: &str = "%Y%m%dT%H%M%SZ";

const RECORDED: &str = "SELECT * FROM extract_manifest ORDER BY extracted_at DESC";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtractId(String);

impl ExtractId {
    pub fn at(at: DateTime<Utc>) -> Self {
        Self(at.format(COMPACT_UTC).to_string())
    }

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

pub async fn recorded(root: &Root) -> Result<Vec<ExtractManifestRow>, ExtractError> {
    let query = Query::new(root.clone());
    query
        .register_by_name(medallion_model::EXTRACT_MANIFEST)
        .await?;
    Ok(query.rows(RECORDED).await?)
}

pub fn is_filled(root: &Root, id: &ExtractId) -> Result<bool, ExtractError> {
    Ok(root
        .dataset(medallion_model::OVERTURE_EXTRACT)
        .for_id(id)?
        .is_filled())
}

pub async fn recorded_as(root: &Root, id: &ExtractId) -> Result<ExtractManifestRow, ExtractError> {
    recorded(root)
        .await?
        .into_iter()
        .find(|recorded| recorded.extract_id == id.to_string())
        .ok_or_else(|| ExtractError::NoSuchExtract { id: id.clone() })
}

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
    #[error("{country} has no boundary in release {release}, so its bbox is unknown")]
    NoCountryBoundary { country: Country, release: String },
    #[error("no extract has been recorded, so there is none to take again")]
    NoExtract,
    #[error("no extract {id} in the manifest")]
    NoSuchExtract { id: ExtractId },
    #[error(
        "extract {id} is already filled in; an extract is immutable, so filling it again \
         would double its rows rather than replace them"
    )]
    AlreadyFilled { id: ExtractId },
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

#[derive(Debug, Clone, PartialEq)]
pub struct Extraction {
    pub id: ExtractId,
    pub bbox: Rect<f64>,
    pub rows: Vec<(OvertureType, usize)>,
}

pub struct Extractor<'a> {
    overture: &'a Overture,
    root: &'a Root,
}

impl<'a> Extractor<'a> {
    pub fn new(overture: &'a Overture, root: &'a Root) -> Self {
        Self { overture, root }
    }

    pub async fn extract(
        &self,
        id: ExtractId,
        country: Country,
        at: DateTime<Utc>,
    ) -> Result<Extraction, ExtractError> {
        self.register_themes().await?;
        let bbox = self.country_bbox(country).await?;
        let rows = self.write_types(&id, country.code(), &bbox, at).await?;
        self.record_extract_as_complete(&id, country, at, &bbox)
            .await?;

        Ok(Extraction { id, bbox, rows })
    }

    pub async fn backfill(
        &self,
        recorded: &ExtractManifestRow,
        at: DateTime<Utc>,
    ) -> Result<Extraction, ExtractError> {
        let id = ExtractId::new(recorded.extract_id.clone())?;
        let bbox = bbox_of(recorded);

        let opened = self.overture.release().id();
        if opened != recorded.release {
            return Err(ExtractError::WrongRelease {
                id,
                recorded: recorded.release.clone(),
                opened: opened.to_string(),
            });
        }
        if is_filled(self.root, &id)? {
            return Err(ExtractError::AlreadyFilled { id });
        }

        self.register_themes().await?;
        let rows = self.write_types(&id, &recorded.country, &bbox, at).await?;

        Ok(Extraction { id, bbox, rows })
    }

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

    async fn write_types(
        &self,
        id: &ExtractId,
        code: &str,
        bbox: &Rect<f64>,
        at: DateTime<Utc>,
    ) -> Result<Vec<(OvertureType, usize)>, ExtractError> {
        let in_bbox = bbox_overlaps(bbox);
        let of_country = of_country(code);
        let rail = format!(
            "subtype = 'rail' AND {class} AND {in_bbox}",
            class = excluding_classes(),
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
            (OvertureType::WATER, "water", in_bbox.clone()),
            (
                OvertureType::CONNECTOR,
                "connectors",
                referenced_connectors(&in_bbox, &rail),
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
            .dataset(medallion_model::OVERTURE_EXTRACT)
            .for_id(id)?
            .partition("theme", overture_type.theme)?
            .partition("type", overture_type.name)?
            .append_geo_stream(at, stream)
            .await?
            .rows)
    }

    async fn country_bbox(&self, country: Country) -> Result<Rect<f64>, ExtractError> {
        let batches = self
            .overture
            .sql(&format!(
                "SELECT MIN(bbox.xmin) AS min_lon, MIN(bbox.ymin) AS min_lat,
                        MAX(bbox.xmax) AS max_lon, MAX(bbox.ymax) AS max_lat
                 FROM division_area
                 WHERE {DIVISION_ID} = '{}'",
                medallion_model::division_id(country)
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

    async fn record_extract_as_complete(
        &self,
        id: &ExtractId,
        country: Country,
        at: DateTime<Utc>,
        bbox: &Rect<f64>,
    ) -> Result<(), ExtractError> {
        let row = ExtractManifestRow {
            extract_id: id.to_string(),
            extracted_at: at,
            release: self.overture.release().id().to_string(),
            country: country.code().to_string(),
            min_lon: bbox.min().x,
            min_lat: bbox.min().y,
            max_lon: bbox.max().x,
            max_lat: bbox.max().y,
        };
        self.root
            .rows_of::<ExtractManifestRow>()
            .append_rows(at, &[row])
            .await?;
        Ok(())
    }
}

fn bbox_of(recorded: &ExtractManifestRow) -> Rect<f64> {
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

fn of_country(code: &str) -> String {
    format!("country = '{code}'")
}

fn bbox_overlaps(extract_bbox: &Rect<f64>) -> String {
    format!(
        "bbox.xmin <= {max_lon} AND bbox.xmax >= {min_lon}
         AND bbox.ymin <= {max_lat} AND bbox.ymax >= {min_lat}",
        min_lon = extract_bbox.min().x,
        min_lat = extract_bbox.min().y,
        max_lon = extract_bbox.max().x,
        max_lat = extract_bbox.max().y,
    )
}

fn referenced_connectors(in_bbox: &str, rail: &str) -> String {
    format!(
        "{in_bbox}
         AND id IN (
           SELECT DISTINCT elem['connector_id']
           FROM (SELECT UNNEST(s.connectors) AS elem FROM segments AS s WHERE {rail}) AS refs
         )"
    )
}

fn excluding_classes() -> String {
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
    use std::path::Path;

    use chrono::TimeZone;
    use sedona::context::SedonaContext;

    use super::*;
    use crate::overture::Release;

    const UNPLACEABLE: &str = "ZZ";

    fn bbox() -> Rect<f64> {
        Rect::new(Coord { x: 5.8, y: 47.2 }, Coord { x: 15.1, y: 55.1 })
    }

    fn manifest_row(id: &str, hour: u32, release: &str) -> ExtractManifestRow {
        ExtractManifestRow {
            extract_id: id.to_string(),
            extracted_at: Utc.with_ymd_and_hms(2026, 7, 27, hour, 0, 0).unwrap(),
            release: release.to_string(),
            country: Country::Germany.code().to_string(),
            min_lon: bbox().min().x,
            min_lat: bbox().min().y,
            max_lon: bbox().max().x,
            max_lat: bbox().max().y,
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

    #[tokio::test]
    async fn the_recorded_extractions_come_newest_first() {
        let (_tmp, root) = store_recording(&[
            manifest_row("20260727T090000Z", 9, "2026-05-21.0"),
            manifest_row("20260727T193628Z", 19, "2026-06-17.0"),
        ])
        .await;

        let recorded = recorded(&root).await.expect("the recorded extractions");

        assert_eq!(
            recorded,
            vec![
                manifest_row("20260727T193628Z", 19, "2026-06-17.0"),
                manifest_row("20260727T090000Z", 9, "2026-05-21.0"),
            ]
        );
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

    #[tokio::test]
    async fn a_store_with_no_manifest_has_no_extraction_to_take_again() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let err = recorded(&Root::new(tmp.path())).await;

        assert!(matches!(
            err,
            Err(ExtractError::Query(
                medallion::QueryError::NoSuchDataset { .. }
            ))
        ));
    }

    #[tokio::test]
    async fn an_extract_already_in_the_store_is_not_taken_again() {
        let recorded = manifest_row("20260727T193628Z", 19, "2026-06-17.0");
        let (_tmp, root) = store_recording(std::slice::from_ref(&recorded)).await;
        let extract = root
            .dataset(medallion_model::OVERTURE_EXTRACT)
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

        assert!(matches!(err, Err(ExtractError::AlreadyFilled { .. })));
    }

    async fn mirror_holding_one_row_of_each_type(dir: &Path, release: &str) {
        let ctx = SedonaContext::new();
        let row_bbox = "{xmin: 13.0, xmax: 13.1, ymin: 52.0, ymax: 52.1}";
        // Geometry comes first in every select: a later position makes the scan panic in
        // sedona's spatial filter, https://github.com/apache/sedona-db/issues/389.
        let geometry = "ST_GeomFromText('POINT (13.05 52.05)') AS geometry";
        let of_type = [
            (
                OvertureType::DIVISION_AREA,
                format!("SELECT {geometry}, 'area-1' AS id, '{UNPLACEABLE}' AS country"),
            ),
            (
                OvertureType::DIVISION,
                format!("SELECT {geometry}, 'division-1' AS id, '{UNPLACEABLE}' AS country"),
            ),
            (
                OvertureType::SEGMENT,
                format!(
                    "SELECT {geometry}, 'segment-1' AS id, 'rail' AS subtype, \
                     CAST(NULL AS VARCHAR) AS class, \
                     [{{connector_id: 'connector-1'}}] AS connectors, {row_bbox} AS bbox"
                ),
            ),
            (
                OvertureType::WATER,
                format!("SELECT {geometry}, 'water-1' AS id, {row_bbox} AS bbox"),
            ),
            (
                OvertureType::CONNECTOR,
                format!("SELECT {geometry}, 'connector-1' AS id, {row_bbox} AS bbox"),
            ),
        ];

        for (overture_type, select) in of_type {
            let at = dir
                .join(release)
                .join(format!("theme={}", overture_type.theme))
                .join(format!("type={}", overture_type.name));
            std::fs::create_dir_all(&at).expect("a mirrored type");
            ctx.sql(&format!(
                "COPY ({select}) TO '{}' STORED AS PARQUET",
                at.join("part-0.parquet").display()
            ))
            .await
            .expect("write the mirrored rows")
            .collect()
            .await
            .expect("finish the write");
        }
    }

    #[tokio::test]
    async fn an_extract_recorded_for_a_country_the_store_cannot_place_is_filled_in() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let mut recorded = manifest_row("20260727T193628Z", 19, "2026-06-17.0");
        recorded.country = UNPLACEABLE.to_string();
        mirror_holding_one_row_of_each_type(mirror.path(), &recorded.release).await;
        let (_tmp, root) = store_recording(std::slice::from_ref(&recorded)).await;
        let overture = Overture::open(Release::mirrored(&recorded.release, mirror.path()));

        let extraction = Extractor::new(&overture, &root)
            .backfill(&recorded, Utc::now())
            .await
            .expect("fill the extract in");

        assert_eq!(extraction.id.to_string(), recorded.extract_id);
        for (overture_type, rows) in &extraction.rows {
            assert_eq!(*rows, 1, "{}/{}", overture_type.theme, overture_type.name);
        }
        let segments = root
            .dataset(medallion_model::OVERTURE_EXTRACT)
            .for_id(&extraction.id)
            .expect("the id the manifest gave it")
            .partition("theme", OvertureType::SEGMENT.theme)
            .expect("the theme")
            .partition("type", OvertureType::SEGMENT.name)
            .expect("the type");
        assert!(segments.is_filled(), "{}", segments.dir().display());
    }

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

    #[test]
    fn the_bbox_predicate_keeps_rows_that_overlap_it() {
        let predicate = bbox_overlaps(&bbox());

        assert!(predicate.contains("bbox.xmin <= 15.1"));
        assert!(predicate.contains("bbox.xmax >= 5.8"));
        assert!(predicate.contains("bbox.ymin <= 55.1"));
        assert!(predicate.contains("bbox.ymax >= 47.2"));
    }

    #[test]
    fn connectors_are_restricted_to_those_rail_segments_refer_to() {
        let predicate = referenced_connectors(&bbox_overlaps(&bbox()), "subtype = 'rail'");

        assert!(predicate.contains("UNNEST(s.connectors)"));
        assert!(predicate.contains("subtype = 'rail'"));
        assert!(predicate.contains("bbox.xmin <= 15.1"));
    }

    #[test]
    fn excluded_classes_are_filtered_out_and_nulls_kept() {
        assert!(
            !EXCLUDED_CLASSES.is_empty(),
            "expects at least one exclusion"
        );
        let filter = excluding_classes();

        assert!(filter.starts_with("coalesce(class, '') NOT IN ("));
        for class in EXCLUDED_CLASSES {
            assert!(filter.contains(&format!("'{class}'")));
        }
    }
}
