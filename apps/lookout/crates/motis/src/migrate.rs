//! Tiny schema migrations: add a column to an existing table so a db created by an earlier
//! schema gains it without a manual `ALTER`.

use rusqlite::Connection;

/// Add `column decl` to `table` if it isn't already present. `table`/`column`/`decl` are
/// code constants, never user input.
pub(crate) fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    let present = names.any(|n| n.map(|n| n == column).unwrap_or(false));
    if !present {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}
