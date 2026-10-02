# Deriving the water crossings in Rust

*Assessment of 2026-09-27, from the notebook as it stands after the two-country change, the
crates the workspace already resolves, and the pinned SedonaDB source. Nothing has been
ported and no port has been measured. It records what the evidence rules in and out, and what
to measure first. Whatever survives a port belongs in [architecture.md](architecture.md) and
[medallion.md](medallion.md); the rest goes stale.*

The water crossings are the one derivation written in Python.
[architecture.md](architecture.md) states the rule it breaks and why: "Rust derives; Python
reads", and "the water crossings derivation stays a marimo notebook, because the work is
spatial SQL and iteration on it is visual". This asks what removing that exception would take,
in whole or in part, and what the store's own build can do today.

## What the notebook asks for

Eight stages, 1,047 lines in `v10.py` and 246 in the two modules beside it.

| Stage | Work | Rows it starts from, DE and GB |
| --- | --- | --- |
| Region | Union of a country's two division areas, and its bbox | 2 and 2 |
| Rail | Scan, bbox prefilter, then clip to the region | 239,614 and 55,347 |
| Water | Scan, bbox prefilter, then range-join to rail bboxes | 3,110,307 and 1,120,122 |
| Crossings | Intersect each rail geometry with each water geometry it meets | — |
| Parts | Split to parts, measure each in metres, locate along the segment, drop the blocked and the redundant | — |
| Cluster | Pairs within a merge distance, connected components, one representative each | — |
| Ids | A composite id per crossing and a four-byte digest of it | — |
| Write | Through `lookout_medallion`, which is the Rust writer already | 5,760 rows today, DE alone |

## This build can do four fifths of the SQL, and not the part that matters

SedonaDB 0.4.0 registers 138 `ST_` functions. Its default features include `geos`, `tg`,
`s2geography` and `spatial-join`; the workspace takes `default-features = false` with
`["aws", "geo"]`, which the comment beside the pin explains — the `geos` backend links a
system GEOS library. The notebook uses 17 functions, and the split falls where the C
dependencies are.

| Function | Implemented in | Available here |
| --- | --- | --- |
| `ST_Dump`, `ST_GeometryType`, `ST_IsEmpty`, `ST_AsWKB`, `ST_X`, `ST_Y`, `ST_XMin`, `ST_XMax`, `ST_YMin`, `ST_YMax` | `sedona-functions`, backend-independent | yes |
| `ST_Centroid`, `ST_Length`, `ST_Intersects`, `ST_Union_Agg` | `sedona-geo`, pure Rust | yes |
| `ST_Intersection`, `ST_Contains` | `c/sedona-geos`, links GEOS | no |
| `ST_LineLocatePoint` | `c/sedona-s2geography`, links S2 | no |
| `ST_Transform` | `c/sedona-proj`, links PROJ | no |

The four absent functions are the ones that make a crossing a crossing: where the rail meets
the water, how far along the segment it happens, whether a point crossing sits inside a
polygon already counted, and how long the overlap is in metres. No GEOS is installed on this
machine, and `geos` appears in no lockfile entry, so none of the four can be reached from SQL
in the store's build as it stands.

## The geometry is available; the SQL formulation is not

Each missing function has a pure-Rust equivalent, and every crate below is already resolved in
`Cargo.lock`.

| What the SQL does | The Rust call |
| --- | --- |
| Intersect a line with a polygon | `geo::BooleanOps::clip`, which clips a `MultiLineString` to a polygon |
| Intersect a line with a line | `geo::algorithm::sweep::Intersections`, a Bentley-Ottmann pass yielding each crossing pair and its intersection |
| Contains | `geo::Contains`, which already places a GPS fix in a country |
| Locate a point along a line | `geo::LineLocatePoint` |
| Length in metres | `geo::Length` over a geometry projected with `proj4rs`, which already writes the projected column |
| Union the areas of a division | `geo::unary_union` |
| Pairs within a merge distance | `rstar`, an R-tree, in place of `scipy.spatial.cKDTree.query_pairs` |
| Connected components over those pairs | `petgraph` 0.8.3, in place of `scipy.sparse.csgraph` |

A Rust derivation therefore needs no C dependency, on one condition: the geometry moves out of
SQL and into Rust code. What is unavailable is the *formulation*, not the capability.

## The join is to be designed, not translated

The water stage is a four-inequality range join — each water bbox against each rail
bbox — and DuckDB executes that with a range join. DataFusion 52.5 offers a hash join and
a sort-merge join, both needing an equality key; a nested loop join for an arbitrary
predicate; and a piecewise merge join whose own documentation says it "is currently
experimental" and "only evaluates single range filter", chosen "when there is only one
comparison filter". Four comparisons do not qualify, so the plan is a nested loop.

The cross products that would then be evaluated, taking the extract counts as the upper bound:
7.45 × 10¹¹ pairs for DE and 6.20 × 10¹⁰ for GB. The notebook's own prefilter to the region
bbox cuts both before the join, and by how much is unmeasured, but no constant factor
rescues the shape.

The port is therefore not a transliteration. An R-tree over the water bboxes, queried once
per rail segment, is what the SQL emulates with inequalities — and it is what SedonaDB's own
spatial join does, building an index with `geo-index` and refining with GEOS. Refinement here
would be `geo` instead.

## What moves, and what stays

| Piece | Cost | Where it lands |
| --- | --- | --- |
| The scans and the clip to the region | Low: SedonaDB reads the GeoParquet already, and `ST_Intersects` is available | SQL, in Rust |
| The crossing geometry, the measurement and the pruning | The bulk of the work, and a join to design | Rust code over `geo`, `proj4rs` and `rstar` |
| The clustering and the ids | Modest: 160 lines of `crossing_ids.py`, with tests already | Rust code over `rstar` and `petgraph` |

The visual half stays in Python: 397 of the notebook's 968 cell lines are `lonboard` maps,
marimo sliders and the test-case picker. The merge distance is tuned by looking at a map, and a
binary cannot answer whether a cluster looks right. A port that took the derivation and left
the exploration would keep that half, reading the store as every other notebook does.

## What a port has to prove

- **The same rows for DE at the same tuning.** 5,760 crossings today, and the ids stable, since
  a moved id renames a row the device and the browser already carry.
- **The test cases still pass**, the three in `test_cases.geojson` and whatever GB adds.
- **A runtime worth the change.** The notebook's cost is unmeasured, which is why it heads the
  list below.

## Ruled out, and deferred

- **Enabling SedonaDB's `geos` backend.** It wants a system GEOS on every machine and in CI,
  and the `spatial-join` feature is not separable from it: `rust/sedona-spatial-join` lists
  `geos`, `sedona-geos` and `sedona-tg` as ordinary dependencies, not optional ones. Revisit
  when GEOS is installed for another reason — the comment beside the pin already anticipates
  that — and note that it would then also buy the indexed spatial join.
- **Driving DuckDB from Rust.** It is already a dependency, for the multi-engine test, linking
  the system libduckdb. Its spatial extension is a runtime download into `~/.duckdb`, which is
  what failed in this sandbox today. This moves the language while leaving the engine, and the
  engine is the part the notebook depends on.
- **Splitting the derivation across SQL and Rust code in one pass**, joins in one and geometry
  in the other. The notebook already does exactly that, in a place where the result can be
  seen.

## What to measure first

Ranked by information per unit of effort.

1. **Time the notebook's stages**, on the run that produces the two-country counts. Which stage
   dominates decides whether a port is about the join or about the geometry, and nothing else
   here is worth doing first.
2. **Count candidate pairs from an R-tree over one country's water bboxes**, in Rust, with
   no geometry work behind it. It answers the join question directly, and `rstar` is already
   resolved.
3. **Reproduce one test case with `geo`**: clip a handful of rail segments against the water in
   a case bbox and compare the parts with what the notebook wrote. It tests the one assumption
   the table above rests on, that `clip` and `sweep` agree with GEOS on this data.

## References

- [SedonaDB](https://github.com/apache/sedona-db), tag `apache-sedona-db-0.4.0` — the feature
  list in `rust/sedona/Cargo.toml`, the per-backend registration under `rust/sedona-geo`,
  `c/sedona-geos`, `c/sedona-proj` and `c/sedona-s2geography`, and the dependencies of
  `rust/sedona-spatial-join`
- [DataFusion](https://datafusion.apache.org/) 52.5,
  `datafusion-physical-plan/src/joins/piecewise_merge_join/exec.rs` — which predicates the
  range join accepts, and that it is experimental
- [geo](https://docs.rs/geo/0.31.0/geo/) 0.31 — `BooleanOps::clip`, `unary_union`,
  `algorithm::sweep::Intersections`, `LineLocatePoint`, `Contains`, `Length`
- [rstar](https://docs.rs/rstar) and [petgraph](https://docs.rs/petgraph) — the R-tree and the
  union-find that replace `scipy.spatial` and `scipy.sparse.csgraph`
- [architecture.md](architecture.md) on Rust deriving and Python reading, and
  [medallion.md](medallion.md) on the multi-engine rule and on writing silver from another
  language
