# Current Slice: minimal predictor and evaluation framework 

### Target

We should now have enough to put together a first minimal predictor that is based solely on crow-flies distance, and also to evaluate how good it is.

### Straw man

The essential idea here is to use our collected traces along with known water crossings to both act as source data and as measurement.

#### Sessionisation

So, we first find all gps readings from real traces and "sessionise" them. This effectively boils down to breaking the data from the same device into sessions whenever:
1. There is an explicit `StartSession` message
2. There is a gap of N minutes between successive readings (N = 10 minutes probably good enough)

#### Water crossings per session

Then, we look at any gps readings in each session that come within M metres of a known water crossing. This is where it is probably a good idea to first normalise a session to include a version of the path that has a CRS in metres (a projected CRS). Same goes for the water crossings dataset. For simplicity, since we are covering Germany, it'd be enough to use a single CRS for now.

Once we have some gps readings for each water crossing for each session, we minimise this to just a single example for each water crossing per trace, using the closest match. This should give us a set of water crossings per session. We treat this as our ground truth.

#### Simple crow-flies predictor

We then implement a simple predictor with a prediction cycle which functions something like:
1. Receive latest GPS reading
2. Find all water crossings within D distance (in metres); remember this for later
3. If we have a previous set of water crossings:
    * find overlap between sets, and compare distances for each pair of old and new, and work out distance delta (delta = new - old)
    * for those where delta is negative (we've gotten closer) calculate velocity
    * emit prediction of wall-clock time we will pass each water crossing based on current distance to water crossing and current velocity towards it

In this simple predictor we are not taking advantage of any speed or heading information in the GPS readings. That will be sensible to include later, but for now we can keep it simple. Later on we'll likely want to include any additional information we have in a sensor-fusion approach but for now we keep it simple.

#### Evaluation framework

We can think of a predictor as attempting to fill in, at each prediction cycle, a 2D space where the y-axis is all the possible water crossings and the x-axis is the time at which the water will be crossed in this session. 

We can run this for each gps reading in each session (each prediction cycle) and then assess as follows:
* precision = for each water crossing, whenever we predicted that we would cross at time T_P, what was the actual T_A, and was it within some tolerance e.g. 30 seconds. Similarly, was it within some distance. Count each of these as a boolean yes/no
* recall = for each water crossing that was really passed in a session, did we make a prediction for it?

This measurement framework and predictor can both likely be improved, but it's probably enough to start with.

### Refactor to Medallion Architecture

I think at this point we need to cleanly separate our bits of data processing and storage into a [medallion architecture](https://motherduck.com/glossary/medallion-architecture/). In this context this means something like:
* landing/external:
    * we can think this as where raw recordings are made in a native live format. So, redis is in here, and things like `motis_poll` should dump stuff here.
* bronze:
    * raw gps and accel sensor readings, recorded live in redis and extracted via `recorder` from live redis
    * motis train samples, recorded via `motis_poll` (sitting in landing) but extracted here via `motis_ingest`
    * point in time extracts from OvertureMaps restricted to our needs e.g. rail/water for Germany
* silver:
    * gps readings sessionised and normalised into standard geometries
    * derived water crossings, represented as an enriched OvertureMaps segments and connector dataset extended/restricted to only what we need
* gold:
    * results of runs evaluating particular predictor versions against silver datasets
    * preparations of data for live usage externally

In principle, as long as we always keep everything in bronze, everything in silver and gold should be rederivable. We shouldn't aim to delete from silver/gold as that slows things down a lot rederiving things, but we can if we need to. bronze must be immutable and versioned, not just append-only.

We also want to start standardising on representions i.e. where possible we should use parquet, but with different biases in each section:
* landing/external:
    * it's ok to continue to store things here optimised for fast in-place update by a single writer e.g. sqlite
* bronze:
    * parquet optimised for quick append of new data; we generally never delete anything from here, and we want to make appends quick and save. The structures should be biased towards sample structure e.g. each unique poll by `motis_poll` should get a timestamp which records when the poll happened and this should be part of the folder structure.
    * we don't need to worry *too much* about lots of small files here because:
        1. We will generally be doing any querying on silver datasets
        2. we don't store things like individual gps/accel entries as files as `recorder. drain` and `motis_ingest` can extract many readings into a batch and we do a write per-batch. So, we end up with a separate file for eacg time ingestion is run as opposed to for each data point.
    * *if we receive it from somewhere* we allow storage here in compact geo formats like [polyline](https://developers.google.com/maps/documentation/utilities/polylinealgorithm) as that's the sort of formats live services use for capturing paths. note that this is not our preferred format for everything in bronze as it doesn't allow representation of everything we care about, but if we receive it from some third-party service then we should store it.
        * if we are generating our own geo formats then we should favour using the same formats as in silver
    * we store extracts from OvertureMaps here largely in the native format they use, but add metadata like when what version of overturemaps was used (in case not already present). this extract may come from a bounding-box restriction we applied so we also add that as metadata
        * the metadata can be stored in a separate table I own, for example containing data of extract, unique extract id, and bounding box. the overture maps table may then only need to be enriched with the extract id as an additional column.
* silver:
    * here parquet is optimised for fast and scalable lookup and processing. this means embedding whatever metadata possible (like bounding boxes) to make queries faster
    * we should use [GeoParquet](https://geoparquet.org) ([v1.1.0](https://geoparquet.org/releases/v1.1.0/)) and ensure everything represents geographic concepts in the same way ([WKB geometry encoding](https://libgeos.org/specifications/wkb/), [simple features](https://www.ogc.org/standards/sfa))
        * there is also GEOMETRY/GEOGRAPHY [geospatial types](https://parquet.apache.org/docs/file-format/types/geospatial/) but these aren't well-supported by many apps / libraries right now
    * when we are extending/subsetting OvertureMaps data and storing here, we should always follow their schemas where possible, even for our own extensions to the data. however, additional, even when we are creating our own data from scratch, we should still follow their schemas as they are likely suitable for what we are doing as well
    * when storing paths or other geo entities, we should *always* have a normalised clean lat/lon representation in a global CRS
        * optionally, we can also eagerly pre-calculate a column in a projected CRS which is most appropriate for the entity. So, for example, for segments in germany we store in a single projected UTM Zone. There are technically multiple zones that cover Germany but it's most practical and useful to have a single zone per country
        * we should ensure CRS is in the GeoParquet metadata (PROJJSON),
* gold:
    * this is where we may produce specialised output formats, like [PMTiles](https://docs.protomaps.com/pmtiles/) / [protomaps](https://protomaps.com/about), intended to be used by live systems. This is also again where things like polylines are allowed/encouraged.
    * where specific formats aren't appropriate, geoarrow ([v0.2](https://geoarrow.org)) should be used to allow easy/fast export/import
        * some things, like kepler.gl, don't support compressed geoarrow so we should use uncompressed for now

The root where this data is stored is ~/Data/geo/lookout/medallion. If this becomes took big, then we'll move to store it on /Volumes/PRO-G40/Data/geo/lookout/medallion (my external drive). Data should be stored in Hive format.

One intent here is to standardis to allow multiple writer/readers, which are different engines, as appropriate i.e. Duckdb, SedonaDB, georust. Any file in silver must be readable by all three engines with no engine-specific handling.

#### Tasks

The main tasks here should be focussed on documenting these patterns and correctinh any conflicting info (e.g. in target.md) and updating the cli's like `motis_poll` and `motis_ingest` to follow them. Further sets of Tasks then need to follow these patterns.

- [x] Write `docs/medallion.md`: the layer definitions above, the root path
      (`~/Data/geo/lookout/medallion`, Hive-partitioned), the per-layer format rules
      (parquet append-shaped in bronze, GeoParquet 1.1 / WKB / simple features in silver,
      PMTiles / uncompressed GeoArrow in gold), and the multi-engine rule (silver stays
      independent of any one engine — currently DuckDB, SedonaDB and georust must all read
      it with no engine-specific handling).
- [x] Fix conflicting statements in `docs/target.md` (which currently says "sensor data is
      persisted in sqlite") and in `.claude/memory/lookout-architecture.md` (the
      "Rust derives tables into `lookout.sqlite`, Python reads" convention) so they point
      at the medallion layout, keeping sqlite explicitly as a landing/external format.
- [x] Define the Hive partitioning keys per dataset up front (e.g. bronze sensors by
      `dataset/ingested_at_date/`, motis polls by poll timestamp, Overture extracts by
      `extract_id`) and record them in `docs/medallion.md` — path layout is the schema
      here and is expensive to change later.
- [x] Add a small shared Rust `medallion` module (paths + layer roots + writer helpers)
      so the CLIs don't each hand-roll path construction, including a shared clap args
      struct giving every CLI the same `--medallion-root` flag and default.
      Note: built as its own `medallion` crate rather than a module in `shared`, so the
      parquet/arrow dependencies stay off the crates that don't need them (`server` in
      particular, which is deployed). Writes go through `object_store`'s `LocalFileSystem`
      + `ParquetObjectWriter` for atomicity, so `write_batches` is async.
      For later bulk writes spanning many partitions (silver rebuilds, gold exports),
      DataFusion's `DataFrameWriteOptions::with_partition_by` derives `key=value`
      directories from column values — worth using there, but it generates its own file
      names, so it doesn't fit the one-file-per-ingestion bronze writes.
- [x] Prove the multi-engine rule with a round-trip test: write one silver GeoParquet from
      Rust, read it back from each engine in use (currently DuckDB, SedonaDB, georust), and
      assert identical geometry + CRS. This is the check that stops silver drifting
      engine-specific.
      Note: `crates/medallion/tests/multi_engine.rs`, in the default nextest profile so it
      runs on every `just test`. DuckDB links the system libduckdb (`brew install duckdb`,
      added to `just prerequisites`, paths in `.cargo/config.toml`) — the bundled build
      needs a C++ toolchain that isn't available in the sandbox.
- [x] Move `motis_poll` to write into landing/bronze: keep the raw polled `TripSegment`
      batch (polylines verbatim, as received) as one parquet file per poll under a
      timestamped Hive path, rather than appending to `data/motis.sqlite`.
      Note: `motis::bronze::SegmentLog` writes it; rows are defined once as a serde struct
      and turned into arrow by `serde_arrow`. `crates/motis/src/store.rs` (sqlite) stays
      for now because `motis_ingest` still reads it — remove it with the ingest task.
- [x] Move `motis_ingest` to read that bronze poll data and write its deduped, decoded
      `train_segment` output as silver GeoParquet (WKB, CRS 84, plus a pre-projected
      UTM-for-Germany column), rather than into `lookout.sqlite`.
      Note: dedup is a plain Rust fold over the bronze rows rather than a SQL window
      function, since the volume is small and it needs no engine. `store.rs`/`migrate.rs`
      are gone with the sqlite log. Projection uses `proj4rs` (pure Rust, no system PROJ)
      via `medallion::Projector`; `visualise` still reads the old `train_segment` sqlite
      table until the task below repoints it.
- [x] Move `recorder drain` output to bronze: one parquet file per drain batch of raw
      gps/accel readings (the lossless `raw` payload stays the source of truth).
      Note: written as four datasets — `raw_sample`, `gps_reading`, `accel_reading`,
      `device_session` — rather than one under a `sensor=` partition, since the sensors
      carry different columns and a dataset is one schema; `medallion.md` updated to
      match. A drain writes in batches of 100 rather than once at the end, bounding what
      a failed write followed by a failed requeue can lose.
- [x] Move the Overture extracts (`transport::enrich` rail, and the water extract from the
      water-crossings notebooks) to bronze in Overture's native shape, with an
      `extract` metadata table (extract id, date, Overture release, bounding box) and an
      `extract_id` column joined onto the extracted rows.

      Decisions taken before starting, as each changes what gets built:

      * **One extraction covers both rail and water**, not one per theme. The two are
        joined to each other (the crossings pipeline range-joins rail envelopes against
        water envelopes), so a single `extract_id` keeps that join within one provenance
        record and one release. `extract_id` is the outermost key, with Overture's own
        `theme=`/`type=` layout below it, so both themes sit under the one extraction.
      * **The extraction window is the country, not the observed bboxes.** `enrich`
        currently derives per-`(device, UTC day)` bboxes and fetches only what intersects
        them; the notebooks extract Germany-wide from the country division's boundary. One
        extraction serving both has to use the wider window, so the window is the country
        bbox and the manifest records it. Narrowing to observed bboxes stays available to
        the silver derivations, which is where a query-shaped subset belongs.
      * **`theme=divisions` is extracted too**, though it is neither rail nor water. The
        country window itself is derived from the `division_area` country boundary, so an
        extraction that doesn't record it can't be re-derived from what it stored; the
        notebooks additionally clip against that boundary and label with `division`
        localities. Leaving divisions out would keep a live S3 dependency in the notebooks
        and defeat the point of recording a release.
      * **Bronze keeps Overture's rows verbatim.** `overture.rs` today flattens Overture's
        `bbox` struct into `min_lon`/`max_lon`/`min_lat`/`max_lat` and converts geometry
        with `ST_AsBinary`. That is silver shaping and does not belong in the bronze
        extract — it moves to the derivation that reads the extract. The only column bronze
        adds is `extract_id`.

      Steps:

      - [x] Define the two datasets in `model`: the extract itself (bronze, keyed
            `extract_id`) and the manifest (bronze, unpartitioned — id, extraction
            instant, Overture release, bbox). This needs two things loosening first: the
            `model` test asserting *every* partition key ends with `_date` (it should
            assert that date-valued keys name their event, not that all keys are dates),
            and `medallion::Dataset`, which offers `on_date` but no way to partition on an
            id.
      - [x] Extract both themes to bronze under one `extract_id`: `theme=transportation`
            rail segments and their connectors, and `theme=base` water, each restricted to
            the country window and written in Overture's shape plus `extract_id`. Write
            the manifest row for the extraction.
            Note: `transport::extract`, driven by the `extract` bin, which takes the
            release from the bucket or from `--mirror`. Two caveats the extract carries,
            both inherent to restricting by envelope rather than by boundary. Connectors
            are kept only where the connector's own point falls in the window, so ~0.4% of
            those a kept rail segment references (2,017 of 464,555 for DE) are absent —
            endpoints of segments clipping the frontier. And a row is kept when its
            envelope *overlaps* the window, so water reaches to lon -4.0 against a window
            starting at 5.9: the North Sea is one polygon. Neither is a defect to fix here;
            silver clips precisely against the boundary.
      - [-] Repoint `enrich`'s input off sqlite: the bboxes it derives come from bronze
            `gps_reading`, not the `gps` table of `data/lookout.sqlite`. Leaving this on
            sqlite would re-establish the dependency this migration exists to remove.
            `transport::archive` (the sqlite reader) and `transport::store` (the sqlite
            `transport` table) both go.
            Moot: `enrich` is removed rather than migrated. Its purpose was to fetch the
            Overture data intersecting the observed bboxes, and the country-wide extract
            now covers that, so migrating it would leave two ways in with nothing to
            choose between them. The narrowing it did — Overture restricted to where we
            have actually been — is still wanted, but it belongs to a silver derivation
            over sessionised fixes rather than to a fetch, and is better built once
            sessions exist. `archive`, `groups` and `store` go with it, taking transport's
            last sqlite dependency; `data/lookout.sqlite`'s existing `transport` table is
            untouched and `visualise` still reads it until repointed below.
      - [x] Point the water-crossings notebooks at the bronze extract instead of S3 or the
            `/Volumes/PRO-G40` Overture mirror, so a rerun is reproducible against a
            recorded release rather than against whatever S3 currently holds. The
            `USE_LOCAL` mirror fallback goes with it.
            Note: done as a new `v8`, not an edit of `v7` — the earlier versions are the
            record of what was run at the time, and rewriting them to read an extract that
            did not exist then would falsify it. v8 pins an `extract_id` and reads the
            manifest for the release behind it, so adopting a later extract is a deliberate
            edit. Its three test cases pass unchanged, which is what says the extract lost
            nothing the pipeline depends on. Note v8 only runs where that extract exists:
            nothing in the repo recreates a *given* id, since a rerun of `just extract`
            makes a new one.
- [ ] Point `visualise/` at the new silver/bronze parquet instead of `lookout.sqlite`,
      confirming the rerun output is unchanged — the regression check that the migration
      lost nothing.
      Note: its `transport` table no longer has a writer, `enrich` having been removed
      above, so that pane is frozen at whatever the last `enrich` run left. "Unchanged
      output" is therefore only a fair check for the sensor panes; the transport pane needs
      a new source rather than a repointing, and the derivation that would feed it is the
      one deferred until sessions exist.
- [ ] Backfill existing `data/lookout.sqlite` and `data/motis.sqlite` content into bronze
      once, so history isn't stranded behind the old format.
- [ ] Decide whether partition columns should be declared with their types rather than
      inferred. SedonaDB auto-discovers them when the reader doesn't set them
      (`listing_table_factory_infer_partitions` defaults to true), typing every one as
      `Utf8View` — so a date partition comes back as a string, and a predicate has to
      compare it as one. Establish first what a date-typed predicate actually does against
      an inferred column (prunes, silently reads everything, or errors); declare the types
      via `GeoParquetReadOptions::with_table_partition_cols` only if that shows a reason to.
- [ ] Delete `docs/2026-07-27-data-engineering-review.md` at the end of this slice. It is a
      dated snapshot of how this store compares to Rust data engineering practice, kept for
      the history of the decision; anything in it still worth following by then belongs in
      `medallion.md` or in a task, and the rest goes stale.

### Sessionisation

#### Tasks 

...

### Water crossings per session

We probably need to here productionise the pipeline we prototyped in apps/lookout/notebooks/water_crossings/v7.py. However, it's ok to keep it as a notebook, or chain of notebooks, for now.

#### Tasks 

...

### Simple crow-flies predictor

This should probably be written in Rust as this will be the beginnings of what we later embed in a live system. So, we may need to put some wrappers around it to make it easy to call from Python as part of the eval framework (see below).

We should start following the [ports and adapters pattern](https://8thlight.com/insights/a-color-coded-guide-to-ports-and-adapters) with this set of changes so that, for example, the predictor is not aware if it is being driven by recorded or live GPS data.

#### Tasks 

...

### Evaluation framework

This should be written in marimo notebooks and try to re-use as much typical evaluation libraries as possible. So, once we've defined our precision/recall definitions I'd like to plug those into standard well-supported python libraries which allows us to define things like F1-score on top.

#### Tasks 

...
