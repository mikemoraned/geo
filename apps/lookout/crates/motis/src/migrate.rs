//! Tiny schema migrations: add a column to an existing table so a db created by an earlier
//! schema gains it without a manual `ALTER`.

use rusqlite::Connection;

/// The `/trip`-enrichment columns added to the `segment` / `train_segment` tables after
/// their initial schema (agency in one change, train number in a later one). Ensuring all
/// of them lets a db from any earlier schema be read/written without a manual `ALTER`.
const ENRICHMENT_COLUMNS: [(&str, &str); 3] = [
    ("agency_id", "TEXT"),
    ("agency_name", "TEXT"),
    ("train_number", "INTEGER"),
];

/// Bring `table` (`segment` or `train_segment`) up to the current schema by adding any
/// missing enrichment column. No-op when the table is absent or already current.
pub(crate) fn ensure_enrichment_columns(conn: &Connection, table: &str) -> rusqlite::Result<()> {
    for (column, decl) in ENRICHMENT_COLUMNS {
        ensure_column(conn, table, column, decl)?;
    }
    Ok(())
}

/// Add `column decl` to `table` if the table exists and doesn't already have the column —
/// a no-op when the table is absent (nothing to migrate) or the column is present.
/// `table`/`column`/`decl` are code constants, never user input.
fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    // An empty column list means the table doesn't exist — nothing to migrate.
    let table_exists = !names.is_empty();
    let has_column = names.iter().any(|n| n == column);
    if table_exists && !has_column {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}
