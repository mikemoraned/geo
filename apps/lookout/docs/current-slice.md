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

The bounding boxes are probably small enough that a live fetch of overturemaps data from S3 will be fast enough, but if needed we have a recent local copy on disk. We should do this as a new `enrich` cli in a new `transport` crate, and we should save in sqlite as a new "transport" table, using whatever direct geo support it has. There are perhaps better geo db but let's stick with sqlite for now unless it makes it really hard.

The visualisation should be done by extending the current cli to add a new track corresponding to the segments. Later on we might want to restrict the segments visualised to be those within some distance of a sample, but we can leave that out for now if it's not trivial to do.

### Decisions

- **Overture is fetched live from public S3** (`s3://overturemaps-us-west-2/release/<release>/theme=transportation/type=segment|connector`), anonymous (`--no-sign-request`, region `us-west-2`). The offline local copy is a non-goal for now.
- **Fetch/query with SedonaDB** ([`apache/sedona-db`](https://github.com/apache/sedona-db)) — a Rust-native, DataFusion/GeoArrow-based single-node engine. Reads Overture GeoParquet from S3 anonymously (`aws.skip_signature=true`, `aws.region=us-west-2`) with automatic bbox row-group pruning. Geometry read out as WKB via `ST_AsBinary(geometry)`.
  - *Chosen over the `duckdb` Rust crate — and we're committed to it regardless of its C/C++ deps.* This isn't about avoiding C++ (both bundle a compiled engine: duckdb = C++, sedona = C/C++ via `geos`/`tg`). SedonaDB wins because it's **Rust-native and DataFusion/GeoArrow-based** — geometry is a first-class Arrow type we operate on in-process — and we anticipate wanting **non-trivial spatial work later** (spatial joins, distance, reprojection) that is far nicer to grow inside an embeddable Rust engine than through duckdb's SQL-string + extension model. We accept the C/C++ build cost as the price of that.
  - *Note (the fetch itself is simple):* this first slice only needs a numeric `bbox.{xmin,xmax,ymin,ymax}` filter and WKB passthrough (Overture already stores `geometry` as WKB), so the actual query is basic — the spatial engine is an investment in what comes next, not something this task leans on.
  - *Build details (researched):* sedona-db is **not on crates.io**, so we pin a **git dependency** to tag `apache-sedona-db-0.4.0` (rev `b6f66a88`). Feature selection is an implementation detail (at minimum `aws` for S3 + a geometry backend); we currently use `["aws", "geo"]` and can add `geos`/`gdal`/`proj` later. The entry point is `sedona::context::SedonaContext` (a wrapper over DataFusion's `SessionContext`); queries return arrow `RecordBatch`es, so our crate uses the **same arrow version sedona pins (57.1)** (it also pins datafusion 52.5, object_store 0.12; edition 2021, MSRV 1.88).
- **Rail segments only**: filter `segment.subtype = 'rail'`; keep only the `connector`s those segments reference. A `subtype`/`class` column is retained so the viz can distinguish rail classes.
- **sqlite geo storage**: geometry as a WKB blob + bbox columns, with an R\*Tree spatial index (rusqlite `rtree` feature) for later "within distance of a sample" queries.

### Tasks

**`transport` crate scaffold + bbox derivation**
- [x] Add a `transport` crate to the workspace (`crates/transport`) with an `enrich` bin (`src/bin/enrich.rs`) and modules under `src/` exposed via `lib.rs`; pull `duckdb` (bundled) and `rtree`-enabled `rusqlite` to `[workspace.dependencies]`. *(rusqlite 0.40 has no `rtree` cargo feature — the R\*Tree module is compiled into the `bundled` SQLite build already, so `bundled` alone suffices; verified a `USING rtree(...)` virtual table works.)*
- [x] Read the `gps` table from the sqlite archive and group rows into `(device_id, UTC day)` groups; derive a bounding box (min/max lat/lon) per group. Unit-test grouping + bbox against captured rows.

**Overture fetch via SedonaDB**
- [x] Add `sedona` as a git dep pinned to tag `apache-sedona-db-0.4.0` (features incl. `aws` + the geometry backends we want, e.g. `geo`/`geos`); pull `sedona` + a matching `arrow` (57.1) into `[workspace.dependencies]` and drop the now-unused `duckdb` workspace dep. Confirm `cargo build -p transport` + a trivial `SedonaContext::new()`. *(Enabled `["aws", "geo"]` only — **dropped `geos`**: the georust `geos` crate links a system GEOS 3.12 that isn't installed here. The pure-Rust `geo` backend covers ST functions for now; enable `geos` later after `brew install geos`. Also renamed the crate `geo` → `transport` to avoid a name clash with georust's `geo` crate that sedona pulls in transitively.)*
- [x] Open a `SedonaContext`, register anonymous S3 (`aws.skip_signature=true`, `aws.region=us-west-2`), and read one `type=segment` partition for a single bbox as a smoke test. `--release` CLI arg with a pinned default; `--db` input/output paths. *(Learnings, key for the next task: bumped `DEFAULT_RELEASE` to `2026-06-17.0` — Overture ages out old releases (S3 only had 2026-05 & -06). Point the reader at the **directory prefix** (trailing `/`), not a `/*` glob (glob fails DataFusion's `.parquet` extension check). **Must filter with a spatial predicate** `ST_Intersects(geometry, ST_SetSRID(ST_GeomFromWKT(envelope), 4326))` so SedonaDB prunes GeoParquet row groups by their bbox covering: this ran in ~1m14s vs ~13min for a numeric `bbox['xmin']` filter, which barely prunes. `ST_SetSRID(…, 4326)` is required — the Overture geometry column is `ogc:crs84` and the WKT constant is CRS-less, which errors without it.)*
- [x] Query rail `segment`s intersecting the bboxes with the spatial predicate above (per-bbox `ST_Intersects` so row-group pruning applies), `subtype = 'rail'`, returning geometry as WKB (`ST_AsBinary`) + `subtype`/`class` + referenced connector ids. Collect to arrow `RecordBatch`es. *(`Overture::rail_segments(&[BBox])` runs **one** query against a single `MULTIPOLYGON` of all the bbox envelopes rather than per-bbox — the partition is scanned once and pruned by the union's covering (fine while the boxes cluster; revisit if devices scatter globally). Returns `id`/`subtype`/`class`/`geometry` (WKB)/`connectors` (the raw list of `{connector_id, at}` — ids extracted in the next task). Live: 691 rail segments across the 4 bboxes in ~1m26s.)*
- [x] Fetch the `connector`s referenced by the kept segments (by id). Union + dedupe segments and connectors across all bboxes on Overture GERS `id`. *(`Overture::rail_connectors(&[BBox])`: a connector is kept when it falls in the window (so the connector partition prunes) **and** its `id` is in the distinct `connector_id`s `UNNEST`ed from the rail segments in the window — `SELECT UNNEST(s.connectors) AS elem … elem['connector_id']` in a derived table (DataFusion names a bare `UNNEST(...)` column, so it must be aliased + bracket-accessed). Dedup is inherent: one combined MULTIPOLYGON query per type, and connector/segment `id`s are unique in their partitions. Live: 691 segments + 1850 connectors in ~2m15s. **Caveat:** the connector spatial filter drops the rare referenced connector sitting just outside the window.)*

**Persist the `transport` table**
- [x] Define the `transport` table (integer rowid PK, `gers_id TEXT UNIQUE`, `kind` segment|connector, `subtype`, `class`, `geom` WKB blob, bbox cols) + an R\*Tree virtual table on the bbox keyed on rowid. Idempotent `INSERT OR IGNORE` on `gers_id`, following the `recorder::store` pattern. *(New `store` module. bbox cols come straight from Overture's `bbox` struct, flattened to `min_lon`/`max_lon`/`min_lat`/`max_lat` in the fetch SQL — so no WKB parsing is needed to fill the table or the R\*Tree; the `connectors` list column was dropped from the segment fetch since connector resolution now lives entirely in `rail_connectors`'s SQL.)*
- [x] Persist fetched rows into `transport`. Test the transform/persist path with captured Overture data (WKB fixtures); validate the live S3 fetch manually via `just enrich` (and/or an `end-to-end`-profile test). *(`Store::insert_segments`/`insert_connectors` decode the arrow batches — robust to string/binary view types via `arrow::compute::cast`. Unit-tested against synthetic batches with real point-WKB fixtures (round-trip, null class, kind, R\*Tree indexing, idempotency). Live end-to-end: `enrich` persisted 691 segments + 1850 connectors (2541 rows, 2541 R\*Tree entries); a second run stored 0/0. `just enrich` recipe still to come in Wiring.)*

**Visualise (extend `visualise/main.py`)**
- [ ] Read the `transport` table; log segments as static `GeoLineStrings` under `/transport/segments` (coloured by subtype) and connectors as `GeoPoints` under `/transport/connectors`, parsing WKB with shapely. Add a transport map pane to the blueprint.
- [ ] Add a python test for the new logging path.
- [ ] *Enrichment on top of the unfiltered version:* optionally restrict the segments logged to those within some distance of a gps sample (a `--near <distance>` flag, off by default). **Hack:** filter using a raw distance in **degrees** rather than doing it properly — the correct approach would reproject to a metres-based CRS and measure true distance, but a degrees threshold is good enough for a first cut. Note this caveat in the code/help text.

**Wiring + tidy**
- [ ] Add a `just enrich` recipe; update READMEs where needed.
- [ ] `cargo fmt`; `just test-no-docker` + python tests pass.