//! The `transport` table: Overture rail segments and their connectors, persisted into
//! the same SQLite archive the recorder writes. Each row carries the geometry as a WKB
//! blob plus its bounding box, and an R\*Tree virtual table indexes those boxes for
//! later "within distance of a sample" queries. Follows the `recorder::store` pattern:
//! idempotent `INSERT OR IGNORE` on the Overture GERS `id`, so re-running `enrich`
//! against the same archive doesn't duplicate rows.

use std::path::Path;

use arrow::array::{Array, BinaryArray, Float64Array, RecordBatch, StringArray};
use arrow::compute::cast;
use arrow::datatypes::DataType;
use rusqlite::{params, Connection};

/// DDL run on open; `IF NOT EXISTS` makes opening an existing archive a no-op. The
/// R\*Tree is a separate virtual table keyed on `transport.rowid`, kept in step by
/// [`Store::insert_row`] (SQLite's R\*Tree can't be a plain index on the table).
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS transport (
  rowid   INTEGER PRIMARY KEY,
  gers_id TEXT NOT NULL UNIQUE,
  kind    TEXT NOT NULL,
  subtype TEXT,
  class   TEXT,
  geom    BLOB NOT NULL,
  min_lon REAL NOT NULL, max_lon REAL NOT NULL,
  min_lat REAL NOT NULL, max_lat REAL NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS transport_rtree USING rtree(
  id, min_lon, max_lon, min_lat, max_lat
);
";

/// Which kind of transport feature a row holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Segment,
    Connector,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Segment => "segment",
            Kind::Connector => "connector",
        }
    }
}

/// Failure persisting to the `transport` table.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("record batch is missing column {0:?}")]
    MissingColumn(String),
}

/// A handle to the SQLite archive's `transport` table.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if absent) the archive at `path` and ensure the `transport`
    /// table + R\*Tree exist.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an in-memory archive with the schema created; discarded on drop. Test-only.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Persist rail `segment` rows from [`crate::overture::Overture::rail_segments`]
    /// batches (`id`/`subtype`/`class`/`geometry`/bbox columns). Returns the number of
    /// rows newly inserted (existing `gers_id`s are ignored).
    pub fn insert_segments(&self, batches: &[RecordBatch]) -> Result<usize, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut inserted = 0;
        for batch in batches {
            let ids = strings(batch, "id")?;
            let subtypes = strings(batch, "subtype")?;
            let classes = strings(batch, "class")?;
            let geoms = binaries(batch, "geometry")?;
            let bbox = BBoxColumns::read(batch)?;
            for i in 0..batch.num_rows() {
                inserted += insert_row(
                    &tx,
                    ids.value(i),
                    Kind::Segment,
                    optional(&subtypes, i),
                    optional(&classes, i),
                    geoms.value(i),
                    bbox.row(i),
                )?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Persist `connector` rows from [`crate::overture::Overture::rail_connectors`]
    /// batches (`id`/`geometry`/bbox columns; no subtype/class). Returns the number of
    /// rows newly inserted.
    pub fn insert_connectors(&self, batches: &[RecordBatch]) -> Result<usize, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut inserted = 0;
        for batch in batches {
            let ids = strings(batch, "id")?;
            let geoms = binaries(batch, "geometry")?;
            let bbox = BBoxColumns::read(batch)?;
            for i in 0..batch.num_rows() {
                inserted += insert_row(
                    &tx,
                    ids.value(i),
                    Kind::Connector,
                    None,
                    None,
                    geoms.value(i),
                    bbox.row(i),
                )?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }
}

/// Insert one feature, idempotent on `gers_id`. On a genuinely new row, mirror its
/// bbox into the R\*Tree keyed on the freshly-assigned rowid; on an ignored duplicate,
/// leave the R\*Tree untouched. Returns 1 if inserted, 0 if ignored. Runs on the caller's
/// `conn` (a batch of these shares one transaction).
fn insert_row(
    conn: &Connection,
    gers_id: &str,
    kind: Kind,
    subtype: Option<&str>,
    class: Option<&str>,
    geom: &[u8],
    bbox: [f64; 4],
) -> Result<usize, StoreError> {
    let [min_lon, max_lon, min_lat, max_lat] = bbox;
    let changed = conn.execute(
        "INSERT OR IGNORE INTO transport
           (gers_id, kind, subtype, class, geom, min_lon, max_lon, min_lat, max_lat)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            gers_id,
            kind.as_str(),
            subtype,
            class,
            geom,
            min_lon,
            max_lon,
            min_lat,
            max_lat
        ],
    )?;
    if changed == 1 {
        conn.execute(
            "INSERT INTO transport_rtree (id, min_lon, max_lon, min_lat, max_lat)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                conn.last_insert_rowid(),
                min_lon,
                max_lon,
                min_lat,
                max_lat
            ],
        )?;
    }
    Ok(changed)
}

/// The four bbox float columns of a fetched batch, read once and indexed per row.
struct BBoxColumns {
    min_lon: Float64Array,
    max_lon: Float64Array,
    min_lat: Float64Array,
    max_lat: Float64Array,
}

impl BBoxColumns {
    fn read(batch: &RecordBatch) -> Result<Self, StoreError> {
        Ok(Self {
            min_lon: floats(batch, "min_lon")?,
            max_lon: floats(batch, "max_lon")?,
            min_lat: floats(batch, "min_lat")?,
            max_lat: floats(batch, "max_lat")?,
        })
    }

    fn row(&self, i: usize) -> [f64; 4] {
        [
            self.min_lon.value(i),
            self.max_lon.value(i),
            self.min_lat.value(i),
            self.max_lat.value(i),
        ]
    }
}

/// A batch column cast to Arrow type `dtype` and returned as the concrete array `A`
/// (which must be the array type `dtype` decodes to). Normalises whatever variant the
/// query produced — `Utf8`/`LargeUtf8`/`Utf8View`, a binary variant, `Float32` — so
/// callers see one type.
fn cast_column<A: Array + Clone + 'static>(
    batch: &RecordBatch,
    name: &str,
    dtype: DataType,
) -> Result<A, StoreError> {
    let array = cast(column(batch, name)?, &dtype)?;
    Ok(array
        .as_any()
        .downcast_ref::<A>()
        .expect("cast yields the array type for its DataType")
        .clone())
}

/// A batch column decoded to `StringArray`.
fn strings(batch: &RecordBatch, name: &str) -> Result<StringArray, StoreError> {
    cast_column(batch, name, DataType::Utf8)
}

/// A batch column decoded to `BinaryArray` (the WKB geometry).
fn binaries(batch: &RecordBatch, name: &str) -> Result<BinaryArray, StoreError> {
    cast_column(batch, name, DataType::Binary)
}

/// A batch column decoded to `Float64Array` (the `Float32` Overture bbox widened).
fn floats(batch: &RecordBatch, name: &str) -> Result<Float64Array, StoreError> {
    cast_column(batch, name, DataType::Float64)
}

fn column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a dyn Array, StoreError> {
    batch
        .column_by_name(name)
        .map(|c| c.as_ref())
        .ok_or_else(|| StoreError::MissingColumn(name.to_string()))
}

/// The string at row `i`, or `None` if that cell is null.
fn optional(array: &StringArray, i: usize) -> Option<&str> {
    (!array.is_null(i)).then(|| array.value(i))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::ArrayRef;

    use super::*;

    /// A little-endian point WKB for `(x, y)` — an opaque blob as far as the store is
    /// concerned, but a real one so the round-trip is meaningful.
    fn point_wkb(x: f64, y: f64) -> Vec<u8> {
        let mut wkb = vec![0x01, 0x01, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&x.to_le_bytes());
        wkb.extend_from_slice(&y.to_le_bytes());
        wkb
    }

    fn count(store: &Store, table: &str) -> i64 {
        store
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .expect("count")
    }

    /// Two rail segments, the second with a null `class`, with bbox columns as the
    /// Overture query returns them (`Float32`, exercising the cast to `Float64`).
    fn segment_batch() -> RecordBatch {
        RecordBatch::try_from_iter(vec![
            (
                "id",
                Arc::new(StringArray::from(vec!["seg-1", "seg-2"])) as ArrayRef,
            ),
            ("subtype", Arc::new(StringArray::from(vec!["rail", "rail"]))),
            (
                "class",
                Arc::new(StringArray::from(vec![Some("standard_gauge"), None])),
            ),
            (
                "geometry",
                Arc::new(BinaryArray::from(vec![
                    point_wkb(11.0, 50.0).as_slice(),
                    point_wkb(11.5, 50.5).as_slice(),
                ])),
            ),
            (
                "min_lon",
                Arc::new(arrow::array::Float32Array::from(vec![11.0f32, 11.4])),
            ),
            (
                "max_lon",
                Arc::new(arrow::array::Float32Array::from(vec![11.1f32, 11.5])),
            ),
            (
                "min_lat",
                Arc::new(arrow::array::Float32Array::from(vec![50.0f32, 50.4])),
            ),
            (
                "max_lat",
                Arc::new(arrow::array::Float32Array::from(vec![50.1f32, 50.5])),
            ),
        ])
        .expect("segment batch")
    }

    fn connector_batch() -> RecordBatch {
        RecordBatch::try_from_iter(vec![
            ("id", Arc::new(StringArray::from(vec!["con-1"])) as ArrayRef),
            (
                "geometry",
                Arc::new(BinaryArray::from(vec![point_wkb(11.0, 50.0).as_slice()])),
            ),
            ("min_lon", Arc::new(Float64Array::from(vec![11.0]))),
            ("max_lon", Arc::new(Float64Array::from(vec![11.0]))),
            ("min_lat", Arc::new(Float64Array::from(vec![50.0]))),
            ("max_lat", Arc::new(Float64Array::from(vec![50.0]))),
        ])
        .expect("connector batch")
    }

    #[test]
    fn insert_segments_populates_transport_and_rtree() {
        let store = Store::open_in_memory().expect("open");
        let inserted = store.insert_segments(&[segment_batch()]).expect("insert");

        assert_eq!(inserted, 2);
        assert_eq!(count(&store, "transport"), 2);
        assert_eq!(count(&store, "transport_rtree"), 2);

        let (kind, subtype, class, geom): (String, String, Option<String>, Vec<u8>) = store
            .conn
            .query_row(
                "SELECT kind, subtype, class, geom FROM transport WHERE gers_id = 'seg-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("row");
        assert_eq!(kind, "segment");
        assert_eq!(subtype, "rail");
        assert_eq!(class.as_deref(), Some("standard_gauge"));
        assert_eq!(geom, point_wkb(11.0, 50.0), "WKB round-trips verbatim");
    }

    /// A null `class` cell is stored as SQL NULL, not the string "null".
    #[test]
    fn null_class_is_stored_as_null() {
        let store = Store::open_in_memory().expect("open");
        store.insert_segments(&[segment_batch()]).expect("insert");

        let class: Option<String> = store
            .conn
            .query_row(
                "SELECT class FROM transport WHERE gers_id = 'seg-2'",
                [],
                |r| r.get(0),
            )
            .expect("row");
        assert_eq!(class, None);
    }

    /// Connectors land with `kind='connector'` and null subtype/class.
    #[test]
    fn insert_connectors_stores_kind_and_null_attributes() {
        let store = Store::open_in_memory().expect("open");
        let inserted = store
            .insert_connectors(&[connector_batch()])
            .expect("insert");

        assert_eq!(inserted, 1);
        let (kind, subtype, class): (String, Option<String>, Option<String>) = store
            .conn
            .query_row(
                "SELECT kind, subtype, class FROM transport WHERE gers_id = 'con-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(kind, "connector");
        assert_eq!(subtype, None);
        assert_eq!(class, None);
    }

    /// Re-inserting the same features is a no-op: `INSERT OR IGNORE` on `gers_id` keeps
    /// one row each, and the R\*Tree isn't duplicated.
    #[test]
    fn reinserting_is_idempotent_on_gers_id() {
        let store = Store::open_in_memory().expect("open");
        store.insert_segments(&[segment_batch()]).expect("first");
        let again = store.insert_segments(&[segment_batch()]).expect("second");

        assert_eq!(again, 0, "no new rows on the second insert");
        assert_eq!(count(&store, "transport"), 2);
        assert_eq!(count(&store, "transport_rtree"), 2);
    }

    /// The R\*Tree indexes each row's bbox under its `transport.rowid`, so a spatial
    /// window query returns the matching rowids.
    #[test]
    fn rtree_indexes_bboxes_by_rowid() {
        let store = Store::open_in_memory().expect("open");
        store.insert_segments(&[segment_batch()]).expect("insert");

        // A window around seg-1's box (11.0..11.1, 50.0..50.1) but not seg-2's.
        let hits: Vec<i64> = store
            .conn
            .prepare(
                "SELECT r.id FROM transport_rtree r
                 WHERE r.min_lon <= 11.05 AND r.max_lon >= 11.05
                   AND r.min_lat <= 50.05 AND r.max_lat >= 50.05",
            )
            .expect("prepare")
            .query_map([], |r| r.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("collect");

        let seg1_rowid: i64 = store
            .conn
            .query_row(
                "SELECT rowid FROM transport WHERE gers_id = 'seg-1'",
                [],
                |r| r.get(0),
            )
            .expect("rowid");
        assert_eq!(hits, vec![seg1_rowid]);
    }
}
