# Current Slice: visualise transport geo data for regions

### Target

To get a sense of what kinds of overlaps we may see with real transport data, fetch data from "transport" overturemaps data, where it overlaps with device data. The intent is to see where we can correspond data with transport segments.

We'll do this visualisation in rerun.

### Straw man implementation / preferences

I suggest:
1. grouping data by id and by day
2. getting gps coords for each group
3. finding the bounding box
4. finding all connectors and segments in overturemaps that intersect those bounding boxes
5. unioning and dedupe those together into a single dataset and save

The bounding boxes are probably small enough that a live fetch of overturemaps data from S3 will be fast enough, but if needed we have a recent local copy on disk. We should do this as a new `enrich` cli in a new `geo` crate, and we should save in sqlite as a new "transport" table, using whatever direct geo support it has. There are perhaps better geo db but let's stick with sqlite for now unless it makes it really hard.

The visualisation should be done by extending the current cli to add a new track corresponding to the segments. Later on we might want to restrict the segments visualised to be those within some distance of a sample, but we can leave that out for now if it's not trivial to do.

### Decisions

- **Overture is fetched live from public S3** (`s3://overturemaps-us-west-2/release/<release>/theme=transportation/type=segment|connector`), anonymous (`--no-sign-request`, region `us-west-2`). The offline local copy is a non-goal for now.
- **Fetch/query with SedonaDB** ([`apache/sedona-db`](https://github.com/apache/sedona-db)) — a Rust-native, DataFusion-based single-node engine. Reads Overture GeoParquet from S3 anonymously (`aws.skip_signature=true`, `aws.region=us-west-2`) with automatic bbox row-group pruning. Geometry read out as WKB.
  - *Moved off the `duckdb` Rust crate:* its `bundled` libduckdb compiles the whole engine from C++ source — a heavy build, and `clang++` turned out to be blocked in this environment. SedonaDB is pure Rust, so no C++ toolchain (only the C linker the recorder already uses).
  - *Reframing:* the fetch doesn't strictly need a spatial engine — Overture stores `geometry` as WKB already (we pass the bytes straight through) and the bbox filter is pure numeric on the `bbox.{xmin,xmax,ymin,ymax}` struct. SedonaDB is chosen anyway as a Rust-native engine purpose-built for Overture-on-S3, giving real `ST_*` functions at no C++ cost should we want them later.
  - *Caveat:* sedona-db is early (0.x, fast-moving); its Rust crates may need pinning as a git dependency rather than crates.io, with some API churn expected.
- **Rail segments only**: filter `segment.subtype = 'rail'`; keep only the `connector`s those segments reference. A `subtype`/`class` column is retained so the viz can distinguish rail classes.
- **sqlite geo storage**: geometry as a WKB blob + bbox columns, with an R\*Tree spatial index (rusqlite `rtree` feature) for later "within distance of a sample" queries.

### Tasks

**`geo` crate scaffold + bbox derivation**
- [x] Add a `geo` crate to the workspace (`crates/geo`) with an `enrich` bin (`src/bin/enrich.rs`) and modules under `src/` exposed via `lib.rs`; pull `duckdb` (bundled) and `rtree`-enabled `rusqlite` to `[workspace.dependencies]`. *(rusqlite 0.40 has no `rtree` cargo feature — the R\*Tree module is compiled into the `bundled` SQLite build already, so `bundled` alone suffices; verified a `USING rtree(...)` virtual table works.)*
- [x] Read the `gps` table from the sqlite archive and group rows into `(device_id, UTC day)` groups; derive a bounding box (min/max lat/lon) per group. Unit-test grouping + bbox against captured rows.

**Overture fetch via SedonaDB**
- [ ] Set up a SedonaDB Rust session that reads Overture GeoParquet from S3 anonymously (`aws.skip_signature=true`, `aws.region=us-west-2`). `--release` CLI arg with a sensible pinned default; `--db` input/output paths.
- [ ] Query `segment`s with `subtype = 'rail'` intersecting the bboxes (OR the per-bbox `bbox.{xmin,xmax,ymin,ymax}` predicates so pushdown applies), returning geometry as WKB + `subtype`/`class` + referenced connector ids.
- [ ] Fetch the `connector`s referenced by the kept segments (by id). Union + dedupe segments and connectors across all bboxes on Overture GERS `id`.

**Persist the `transport` table**
- [ ] Define the `transport` table (integer rowid PK, `gers_id TEXT UNIQUE`, `kind` segment|connector, `subtype`, `class`, `geom` WKB blob, bbox cols) + an R\*Tree virtual table on the bbox keyed on rowid. Idempotent `INSERT OR IGNORE` on `gers_id`, following the `recorder::store` pattern.
- [ ] Persist fetched rows into `transport`. Test the transform/persist path with captured Overture data (WKB fixtures); validate the live S3 fetch manually via `just enrich` (and/or an `end-to-end`-profile test).

**Visualise (extend `visualise/main.py`)**
- [ ] Read the `transport` table; log segments as static `GeoLineStrings` under `/transport/segments` (coloured by subtype) and connectors as `GeoPoints` under `/transport/connectors`, parsing WKB with shapely. Add a transport map pane to the blueprint.
- [ ] Add a python test for the new logging path.
- [ ] *Enrichment on top of the unfiltered version:* optionally restrict the segments logged to those within some distance of a gps sample (a `--near <distance>` flag, off by default). **Hack:** filter using a raw distance in **degrees** rather than doing it properly — the correct approach would reproject to a metres-based CRS and measure true distance, but a degrees threshold is good enough for a first cut. Note this caveat in the code/help text.

**Wiring + tidy**
- [ ] Add a `just enrich` recipe; update READMEs where needed.
- [ ] `cargo fmt`; `just test-no-docker` + python tests pass.