# Reading the store's column types in DuckDB

*Investigation of 2026-11-15, written after the store first held a duration column. It records what
DuckDB reads of the store's types and the options for closing the gap. What holds after a choice
belongs in `docs/medallion.md`. The rest goes stale.*

This doc covers how each engine that reads the store sees a column's type, and where DuckDB sees
less than the others. It is for whoever writes a query over the store in DuckDB, or changes how
medallion types a column.

The findings hold for DuckDB 2.0.0, pyarrow 25.0.0, and the arrow-rs `parquet` crate 57.

## How a column's type reaches a reader

A Parquet file types each column twice:

- **Parquet's own types.** A physical type, such as `INT64`, and an optional logical type, such as
  `TIMESTAMP(MILLIS, UTC)`. Every Parquet reader reads these.
- **The Arrow schema.** An Arrow writer also serialises the column's Arrow type into the file's
  key-value metadata, under the key `ARROW:schema`. Only a reader that decodes that key sees it.

Where Parquet has a logical type for an Arrow type, both agree, and every reader sees the same type.
Where it has none, the Arrow type exists only in `ARROW:schema`.

Parquet has no logical type for a duration
([ARROW-6780](https://issues.apache.org/jira/browse/ARROW-6780)). A `Duration(Millisecond)` column
is therefore written as a bare `INT64`, with no logical type, and the unit lives only in
`ARROW:schema`. DuckDB's `parquet_schema` and `parquet_kv_metadata` functions show both halves of
this for any file in the store.

## What each reader sees

| Column type | DataFusion | pyarrow | DuckDB `read_parquet` |
|---|---|---|---|
| Instant: `Timestamp(Millisecond, UTC)` | timestamp | timestamp | `TIMESTAMP WITH TIME ZONE` |
| Name from a closed set: string | string | string | `VARCHAR` |
| Unsigned count: `UInt64` | `UInt64` | `uint64` | `UBIGINT` |
| Duration: `Duration(Millisecond)` | duration | `duration[ms]` | `BIGINT` of milliseconds |
| Geometry: WKB with GeoParquet metadata | binary, read as geometry by SedonaDB | binary | `GEOMETRY`, with no CRS in the type |

DataFusion and pyarrow decode `ARROW:schema` and return a duration. DuckDB's Parquet reader ignores
`ARROW:schema`, and returns the integer with no unit.

DuckDB converts an Arrow duration correctly when the Arrow type reaches it. A table that pyarrow
reads from the same file, scanned by DuckDB, holds the column as `INTERVAL`. Only DuckDB's Parquet
reader drops the type.

DuckDB reads a geometry column from the file's `geo` metadata, which is GeoParquet's, not Arrow's.
It reads the type, but the type shows no coordinate reference system.

## What DuckDB offers

- **`read_parquet` options.** No option in DuckDB 2.0.0 reads `ARROW:schema`. The `schema` option
  sets a column's type, but it matches columns by Parquet field id. The store's files carry no field
  ids, so a column named in it reads as `NULL`.
- **Releases.** The [2.0.0 release notes](https://github.com/duckdb/duckdb/releases/tag/v2.0.0)
  list Parquet changes for `VARIANT`, `TIME_NS`, and `TIMETZ`. None reads `ARROW:schema`.
- **Extensions.** No [community extension](https://duckdb.org/docs/current/extensions/community_extensions)
  adds it. The `arrow` extension reads and writes the Arrow IPC format, not Parquet's embedded schema
  ([Arrow IPC support in DuckDB](https://duckdb.org/2025/05/23/arrow-ipc-support-in-duckdb)).
- **Issues.** A search of 2026-11-15 found no DuckDB issue that asks for durations from
  `ARROW:schema`. Other readers have the same gap with files of this kind, for example
  [pg_parquet](https://github.com/CrunchyData/pg_parquet/pull/299).

## Parquet's `INTERVAL` type

Parquet has a legacy `INTERVAL` converted type: 12 bytes holding months, days, and milliseconds.
DuckDB reads it as `INTERVAL`. It is no substitute for a duration:

- arrow-rs reads it back as `Interval(DayTime)`, not as a duration
  ([arrow-rs schema mapping](https://dev.materialize.com/api/rust/src/parquet/arrow/schema/primitive.rs.html)).
- arrow-rs writes it only from Arrow's interval types, and refuses a nanosecond part that does not
  fit ([arrow-rs #6299](https://github.imc.re/apache/arrow-rs/pull/6299)).
- A calendar interval and a span of time differ: a month or a day has no fixed length.

Storing intervals would give DuckDB the right type and the other readers the wrong one.

## Options

1. **Generate DuckDB views from medallion's row types.** Medallion knows each dataset's columns,
   and which hold instants, durations, and geometries. It can emit one DuckDB view per dataset,
   which applies each conversion: `to_milliseconds` for a duration, and a CRS for a geometry where
   DuckDB supports one. A query then reads the view rather than the files, and sees every type the
   other readers see. The views change with the row types, so a new dataset version gets its view
   from the same code.
2. **Convert in each query.** Each DuckDB query applies `to_milliseconds` to each duration column
   it reads. This needs no new code. Each query has to know which columns are durations, and a
   query that misses one reads a unitless integer.
3. **Read through pyarrow.** A Python caller reads the files with pyarrow, then queries the Arrow
   table in DuckDB. Every type arrives. It serves Python alone, and reads the files outside DuckDB's
   Parquet reader, so DuckDB's pruning and pushdown into the files are lost.
4. **Change DuckDB.** DuckDB's Parquet reader maps a duration in `ARROW:schema` to `INTERVAL`. This
   fixes every DuckDB reader of every Arrow-written file, and its timing is outside this project's
   control.

Option 1 serves every DuckDB query and keeps the conversions in the code that defines the types.
Option 2 suits a single query, and is what a query does until the views exist.

## Open questions

- Whether DuckDB 2.0.0's `GEOMETRY` type can carry a CRS, and whether a view can set one.
- Whether a view over hive-partitioned files keeps DuckDB's partition pruning on `run_date` and
  `run_id`.
- Where the generated views live, and how a DuckDB session loads them.
