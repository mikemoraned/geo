# A SedonaDB scan panics on a nested column ahead of the geometry

*Assessment of 2026-10-03. It draws on the SedonaDB source at `apache-sedona-db-0.4.1`,
`apache-sedona-db-0.5.0-rc0`, and main at a6128ab (2026-10-01). It also draws on scans of the
store and of a test fixture, on 0.4.1 and 0.5.0-rc0. No one has written a fix or reported the
panic upstream. This records what triggers the panic, why, and the possible actions. The
[medallion README](../crates/medallion/README.md#geometry-a-scan-cannot-read) describes the
workaround in use. Once a release fixes the panic, the rest goes stale.*

Two conditions make a filtered SedonaDB scan of a parquet file panic. The file stores its
geometry with the native parquet `Geometry` logical type, and a struct or list column comes
before the geometry. The files SedonaDB writes in this repo carry the native type: the bronze
Overture extract does, and so does the fixture the extract tests write. The
`transportation/segment` partition has nested columns before its geometry, and a scan of it
panics:

```
thread 'tokio-rt-worker' panicked at rust/sedona-expr/src/spatial_filter.rs:526:66:
index out of bounds: the len is 7 but the index is 18
```

## The smallest reproduction

Three columns, one row, and a filter on a column that is not the geometry. The test panics on
0.4.1 and on 0.5.0-rc0 with `the len is 2 but the index is 2`. When the select puts `geometry`
first, the test passes.

```rust
use datafusion::prelude::{col, lit};

#[tokio::test]
async fn nested_column_before_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.parquet");
    let ctx = sedona::context::SedonaContext::new();
    ctx.sql(&format!(
        "COPY (SELECT 'a' AS id, [1] AS nested, ST_GeomFromText('POINT (0 0)') AS geometry) \
         TO '{}' STORED AS PARQUET",
        path.display()
    ))
    .await
    .unwrap()
    .collect()
    .await
    .unwrap();
    ctx.read_parquet(path.display().to_string(), Default::default())
        .await
        .unwrap()
        .filter(col("id").eq(lit("a")))
        .unwrap()
        .collect()
        .await
        .unwrap();
}
```

## Two functions count columns differently

The panic is in the row-group pruning that reads the native geometry statistics, in
`sedona-geoparquet/src/file_opener.rs`.

| Step | Function | What it counts |
| --- | --- | --- |
| Collect the statistics | `top_level_column_indices`, then `row_group_native_geo_stats` | Parquet leaf columns whose path has one part. A struct or a list has only longer paths, so it contributes none. |
| Look them up | `TableGeoStatistics::try_from_stats_and_schema`, in `sedona-expr/src/spatial_filter.rs` | Arrow fields, through `geometry_column_indices`. A struct or a list counts as one field. |

Each nested column ahead of the geometry therefore moves the lookup one place further than the
statistics reach. The pruning runs only for a file whose parquet schema has a native `Geometry` or
`Geography` column. A file that stores its geometry as plain WKB bytes never reaches this lookup.

| Dataset | Unnested columns | Geometry's arrow index | Reads |
| --- | --- | --- | --- |
| `transportation/segment` | 7 | 18 | no |
| `divisions/division_area` | not counted | 1, ahead of every nested column | yes |
| The reproduction | 2 | 2 | no |
| The reproduction, geometry first | 2 | 0 | yes |

## No version fixes it

| Version | The two functions | Panics |
| --- | --- | --- |
| 0.4.0 | not read | yes, at `spatial_filter.rs:558` |
| 0.4.1 | unchanged | yes, at `spatial_filter.rs:526` |
| 0.5.0-rc0 | unchanged | yes, at `spatial_filter.rs:615` |
| main, a6128ab | unchanged | not run |

No issue or pull request in apache/sedona-db reports this panic. Issue
[#389](https://github.com/apache/sedona-db/issues/389) has the same symptom, an index into the
statistics out of bounds. It concerns a projected schema evaluated against the file schema. Its fix
leaves this path unchanged.

## 0.5.0-rc0 changes nothing else here

A copy of the app built against 0.5.0-rc0 needed no source change. It took arrow and parquet 58.3,
datafusion 54.1, object_store 0.13, geoparquet and the geoarrow crates at 0.8, pyo3-arrow 0.17,
serde_arrow 0.14, and geo 0.33. 490 non-Docker tests passed, and the geo tests passed with the
workaround in place. Without the workaround, both scans panicked as they do on 0.4.1.

## Possible actions

- **Report it upstream**, with the reproduction above. It names both functions and the versions
  checked, and it does not depend on Overture data.
- **Propose a fix upstream.** One fix maps each top-level arrow field to its first leaf column. A
  nested field then gets `GeoStatistics::unspecified()`, and the statistics follow arrow field
  order.
  The pruning already treats an unspecified statistic as matching every predicate, so the change
  prunes nothing a correct implementation would keep.
- **Turn the pruning off for the affected reads.** The opener checks `enable_pruning` before it
  builds a spatial filter. Whether a read option reaches that flag is unchecked. If one does, the
  geometry stays typed, and a bronze scan reads at the cost of no row-group pruning.
- **Write bronze geometry ahead of every nested column.** The extract would then reorder Overture's
  columns, which makes bronze differ from the release it records.
- **Keep the workaround** until a release fixes the panic. It costs a WKB decode in the reader, and
  one fixture that orders its columns differently from a release.

## References

- [SedonaDB](https://github.com/apache/sedona-db), tags `apache-sedona-db-0.4.1` and
  `apache-sedona-db-0.5.0-rc0` — `top_level_column_indices`, `row_group_native_geo_stats` and
  `filter_access_plan_using_native_geostats` in `rust/sedona-geoparquet/src/file_opener.rs`, and
  `TableGeoStatistics` in `rust/sedona-expr/src/spatial_filter.rs`
- [apache/sedona-db#389](https://github.com/apache/sedona-db/issues/389) — the projected-schema
  mistake, closed on 2026-08-07
- [overture.md](overture.md) on the bronze extract, and [medallion.md](medallion.md) on the
  engines that read the store
