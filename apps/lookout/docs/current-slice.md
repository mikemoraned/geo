# Current Slice: Spike on Maps using Kiosk display as an example

### Target

We want ultimately to show some Map context within the App itself. This will involve being able to access Map data live and/or embedded in App (if running on M5). As a way to flush out what kinds of options we have, we'll instead using the Kiosk as an example of Map usage.

So, we'll aim to set it up so that:
* When https://lookout-hom.fly.dev/kiosk/ is shown on a bigger screen (e.g. at least iPad size) then it shows a Map with the current route being replayed in it, zoomed in, so that route fills area, and with a dot which moves showing current position being replayed
  * The App itself should appear on top of this Map, perhaps in top-right, so that we can see the route and the App itself, as it would appear
* When showing on a smaller screen than iPad, it should display as it is now

### Straw Man / Spike goal

Ideally, we'd use something like https://protomaps.com (https://docs.protomaps.com/pmtiles/) as our data format and used as the basis for display. So, we'd produce a PMTile file as part of our Gold stage which both contains the data needed for replay (i.e. positions and timings) as well as anything needed for display of the map. This means that we'd be refactoring the Kiosk to get it's data from a PMTiles file.

For display of the Map itself on the page, we'd use https://maplibre.org which should be able to use the same input format. We'd use it also to do the "zoom to fit" behaviour we need.

### The map artefact

A gold artefact, `map.pmtiles`, that holds everything the kiosk shows and replays. It has three
layers:

- `rail` and `water`: the Overture rail segments and water features that the water crossings
  name, across every country, drawn as the base map.
- `route`: each replayed session as one line, drawn on the base map. A line carries its session's
  id, its crossings count, its bounding box, and the zoom level its samples sit at. The kiosk
  lists the sessions from these lines at the file's lowest zoom level.
- `samples`: each replayed sample as a point, at one zoom level only, read by the kiosk rather
  than drawn.

The artefact holds no land, roads, or place labels, and no crossings. The browser still fetches
`crossings.json` for the predictor.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-11-15.

- **The base map is the Overture rail and water that the water crossings name.** The crossings
  are the join between rail and water, with the visibility filters already applied, so the rail and
  water they name are the features the product is about. Bronze already holds both countries'
  extracts, and the App will later want the same two layers on the M5. The alternatives were a
  Protomaps planet extract (larger, and none of it our own data) and hosted tiles (these test
  nothing about embedding).
- **`pack_map` reads the base map live: the features from `water_crossing_v2`, and their shapes
  from the bronze extract each crossing came from.** A crossing row carries the Overture ids of its
  rail and water, their class and the water's subtype, and the extract's id, which together pin
  the bronze file and the row within it. One SQL join per country, on the Overture id and
  `extract_id`, selects the shapes. `water_crossing_v2` holds the filtering decisions, so the map
  repeats none of them. The alternative was a silver copy of the shapes, under Rejected / deferred.
- **The base map shows each feature whole, across the whole country.** The crossings already
  limit the features to rail meeting water, so no corridor around the replayed routes applies. The
  alternatives were clipping to a 5 km corridor around each route, and keeping whole features that
  come within 5 km of one.
- **The replay reads its samples from the PMTiles, as the Straw Man proposes.** The kiosk stops
  fetching `sessions.json`. The alternative was to keep `sessions.json` for the replay and use the
  tiles for display only.
- **A sample carries its full values as attributes.** These are the session, its position in the
  session, `t`, `lat`, `lon`, `alt`, `acc`, `speed`, and `heading`. A vector tile quantises its
  geometry to the tile grid, so position read back from the geometry loses precision. The
  attributes keep the six decimal places gold writes.
- **The `route` layer lists the sessions.** Neither `tippecanoe` nor `mlt convert` writes metadata
  beyond a name, a description, and an attribution, so the list goes in the tiles, as attributes
  of the line each session already has. The alternatives were JSON in the description field, which
  misuses a free-text field, and rewriting the file's metadata after `mlt convert`, which means
  writing the archive's header and directories ourselves.
- **The samples sit at a single zoom level, and the kiosk fetches every tile that covers a session's
  bounding box at that level.** At one level, each sample sits in exactly one tile, apart from the
  tile buffer, which the kiosk deduplicates on the sample's position in the session. The
  alternative is to put the sessions in the file's metadata, which is also inside the PMTiles but
  bypasses the tile format the spike is meant to test.
- **The tiles are MapLibre Tile (MLT) v1, read by MapLibre's own libraries.** The project prefers
  MapLibre's libraries and formats to Mapbox's. v1 is the stable version, and MapLibre GL JS reads
  it from 5.12.0 with `"encoding": "mlt"` on the source. v2 is experimental and no renderer reads
  it. [2026-11-15-maplibre-tile.md](2026-11-15-maplibre-tile.md) compares MLT with Mapbox Vector
  Tile (MVT).
- **The samples sit at zoom level 6, and `rail` and `water` start at zoom level 7.** A tile holds
  every layer at its zoom level, so a zoom level 6 tile holds only routes and samples, and reading
  a journey fetches no base map. At zoom level 6 each of the five sessions reads one tile. Zoom
  levels 8 and 10 read 12 and 64 tiles in all, for the same 15–22 ms on localhost. Below zoom
  level 7 the map shows routes alone, which a map fitted to one journey does not reach.
- **The map's maximum zoom level is 12.** The kiosk fetches single tiles through range requests,
  so the file's total size costs only git, at 13.9 MB per committed version. Zoom level 12 is
  where the routes already stop. The alternatives were zoom level 10, at 5.2 MB, which caps the
  routes, and zoom level 14, at 33.4 MB, which adds detail a map fitted to a journey rarely
  reaches.
- **`tippecanoe` tiles, `mlt convert` encodes, and Rust writes the input.** A new crate, `map`,
  reads the chosen sessions and the crossings' rail and water through medallion's `Query`, and
  writes them as GeoJSON Text Sequences (one feature per line). `tippecanoe` tiles them into an MVT
  PMTiles file in a temporary directory. `mlt convert` turns that into `map.pmtiles` in
  `gold/artifact=map/version=<run>/`. The MVT file is never kept or served. The alternatives were
  Planetiler, which writes MLT directly but adds a JVM and a second tiling tool, and the Rust
  `pmtiles` crate with our own encoding, which would mean writing our own tile clipping and
  simplification.
- **`pack_map` picks its sessions by calling `session_crossings::gold::choose`, with the
  thresholds `gold-pack-sessions` passes.** The map artefact replaces the sessions artefact as
  what the kiosk replays.
- **The map shows at a viewport at least 768 CSS px wide, which covers an iPad in portrait.** The
  map fills the viewport, and `lookout-predictor` sits over it at the top right. Below 768 px, the
  page creates no map and lays out as it does now.
- **Each journey fits the map to its session's bounding box.** A GeoJSON source moves the current
  position's dot, one update for each sample sent.
- **The page vendors MapLibre GL JS, `pmtiles`, and `@maplibre/mlt` into `static/vendor` through
  `just vendor`, each with its version in the file name, as d3 is vendored.** The pages have no build step. MapLibre
  decodes tiles for drawing but gives a page no way to read a tile's features, so the replay
  decodes its tiles with `@maplibre/mlt`'s `decodeTile`. The alternative, `@mapbox/vector-tile`
  with `pbf`, reads only MVT. The published package is not importable without a bundler, so the
  vendored file is esm.sh's bundle of it, `maplibre-mlt-1.3.0.js`.
- **The slice upgrades no dependency.** The `map` crate reads through the `Query` that
  `session_crossings` already uses, and none of what it adds appears in the `just outdated`
  report.

#### Consequences elsewhere

- `docs/medallion.md` describes gold as derived from silver. The map artefact also reads an
  upstream reference extract in bronze, for the shapes of the features silver names.
- The derivation graph in `docs/architecture.md` gains edges from `water_crossing_v2` and the
  Overture extract to the map.
- `sessions.version`, `gold-pack-sessions`, `kiosk-sessions`, and the Dockerfile's
  `SESSIONS_VERSION` lose their only consumer once the kiosk reads the map.
- Building tiles needs `tippecanoe` and the `mlt` CLI on the machine that packs gold. The deploy
  needs neither, as it copies the committed artefact.

#### Rejected / deferred

- **Land, roads, and labels underneath.** These would come back if the bare rail and water fail to
  place a route. A Protomaps extract would then go under the Overture layers.
- **A corridor around the replayed routes.** It would come back if the whole-country base map is
  too large to commit, or if the App shows a map and wants rail and water beyond what the crossings
  name.
- **Reading samples through MapLibre's `querySourceFeatures`.** It returns only the features in the
  tiles MapLibre has loaded for display, clipped to them, so a journey would read differently at
  each zoom level.
- **Silver datasets `rail_v1` and `water_v1` holding the features the crossings name.** They would
  copy bronze's shapes, filtered by ids that `water_crossing_v2` already holds, so the filtering
  decisions live in silver either way. They cost two row types, a recipe, and a write through
  `medallion::write_table` from hand-built batches, since the typed writer takes only datasets
  partitioned by country and date. They come back if a second reader, such as the App, needs the
  same features and a shared function over the join does not serve it.
- **Clipping the base map to each country's boundary.** The map reuses the crossings' choice of
  features, and a clip adds a second choice. Measured in `notebooks/maps/base_map_size.py`,
  clipping to the land area takes GB's water from 26.4 MB to 15.0 MB of WKB, nearly all of it
  ocean. It changes little elsewhere, and it empties 16 rail segments and 18 water features that
  crossings name. Clipping to the maritime area saves almost nothing. It comes back if the ocean
  polygons make the artefact too large.

### Observations

2026-11-15, from one session of 300 samples, packed at zoom level 8:

- **`@maplibre/mlt` 1.3.0 is not importable without a bundler as published.** 337 of its 339
  relative imports omit the `.js` extension, and it imports `@mapbox/point-geometry` by package
  name. esm.sh's bundle (`?bundle&target=es2022`) is one 84 KB ES module with no imports, and
  exports `decodeTile`.
- **`decodeTile(bytes)` returns one `FeatureTable` per layer.** Its `getFeatures()` returns
  `{ id, geometry, properties }` for each feature, with the geometry in tile-grid integers.
- **Every sample's values come back exactly, except a null.** tippecanoe drops a null attribute,
  so it decodes as `undefined`. Of the 300 samples, 20 have a null `speed` or `heading`. The
  replay reads a missing attribute as null.
- **The tile buffer duplicates 24 of the 300 samples** across the session's three tiles.
  Deduplicating on `seq` leaves 300.
- **`mlt convert` keeps tippecanoe's metadata, and stores its tiles uncompressed.** The header's
  tile type is `0x06` and its tile compression is none. The MLT file is 43 KB against the MVT
  file's 20 KB, which tippecanoe gzips.
- **The server answers a range request for a `.pmtiles` file with 206 and the bytes asked for,
  uncompressed, even to a client that takes gzip.** tower-http's `CompressionLayer` skips any
  response that carries `Content-Range`. A whole-file request is compressed, and loses its
  `Accept-Ranges` header.
- **MapLibre GL JS 6.18.0 ships ES modules a page can import, and `pmtiles` 4.5.0 does not.**
  MapLibre's main module and its worker module are self-contained, and it exports
  `setWorkerUrl`. `pmtiles` imports `fflate` by package name, so it is vendored as esm.sh's
  bundle. `pmtiles` names tile type 6 `Mlt`, and its MapLibre protocol passes such a tile's bytes
  through unchanged.
- **Safari reads every sample back from the tiles exactly.** In a throwaway file holding the five
  sessions of sessions version `20261031T152101325Z`, 1,841 samples, the kiosk's reader returned
  each session's crossings count and every sample identical to `sessions.json`. The file holds 28
  tiles in 171 KB, against 105 KB for tippecanoe's gzipped MVT file.
- **Sessions version `20261031T152101325Z` gives every journey a new id.** The journey `0db3e8fd…`
  in earlier versions is `533c9d61…` there, with the same 41 crossings and 300 samples. Commit
  `fb324946` derived the session namespace from its name, which changed every session's id, and
  `31d91dde` packed this version from the result.
- **The `samples` zoom level changes the number of tiles read, not the file's size or the read
  time.** Over the five sessions:

  | Zoom level | MLT file | gzipped MVT file | Tiles read per session | Median read in Safari |
  | --- | --- | --- | --- | --- |
  | 6 | 157 KB | 90 KB | 1, 1, 1, 1, 1 | 15 ms |
  | 8 | 171 KB | 105 KB | 2, 4, 3, 1, 2 | 16 ms |
  | 10 | 196 KB | 131 KB | 24, 12, 16, 4, 8 | 22 ms |

  Each read covers every journey, from localhost, with a URL the browser had not cached.
- **`mlt convert` writes a `.pmtiles` input only to a `.pmtiles` or `.mbtiles` output.** Loose
  `.mlt` tiles come from converting tippecanoe's directory output (`-e`).

2026-11-15, from `water_crossing_v2` derived by `just init`:

- **Every rail and water id a crossing names exists in that crossing's extract, and no id
  appears in two countries.** Each country's crossings come from one extract.

  | Country | Crossings | Rail segments | Rail WKB | Water features | Water WKB |
  | --- | --- | --- | --- | --- | --- |
  | DE | 5,749 | 3,640 | 6.9 MB | 2,984 | 12.6 MB |
  | GB | 4,505 | 2,283 | 4.9 MB | 2,345 | 26.4 MB |

- **A few ocean polygons hold most of the water's bytes.** GB's 36 ocean polygons take 18.9 MB,
  and the largest is 3.2 MB. Rivers take 8.9 MB in DE and 5.2 MB in GB.

2026-11-15, from `pack_map` over the five sessions, 5,923 rail segments, and 5,329 water
features:

- **Each zoom level beyond 10 adds about 60% to the file.**

  | Maximum zoom level | MVT file, gzipped | MLT file |
  | --- | --- | --- |
  | 10 | 4.2 MB | 5.2 MB |
  | 11 | 7.1 MB | 8.6 MB |
  | 12 | 11.7 MB | 13.9 MB |
  | 13 | 19.0 MB | 21.6 MB |
  | 14 | 30.5 MB | 33.4 MB |

- **No tile a client fetches exceeds 108 KB.** Measured in `notebooks/maps/map_tiles.py`, over
  the zoom level 12 file's 17,862 tiles:

  | Zoom level | Tiles | Median | Largest |
  | --- | --- | --- | --- |
  | 0–5 | 13 | 0.4 KB | 0.8 KB |
  | 6 | 2 | 66.2 KB | 107.6 KB |
  | 7 | 39 | 11.5 KB | 31.8 KB |
  | 8 | 119 | 6.6 KB | 31.2 KB |
  | 9 | 403 | 2.6 KB | 52.6 KB |
  | 10 | 1,374 | 1.0 KB | 50.5 KB |
  | 11 | 4,228 | 0.5 KB | 53.1 KB |
  | 12 | 11,684 | 0.2 KB | 54.5 KB |

  The zoom level 6 tiles hold every sample of the sessions they cover, so reading a journey
  fetches at most 108 KB.

### Tasks

#### Phase 1: one session through the tiles

The two unknowns are reading samples back out of a tile in the browser, and serving range
requests through the server. This phase takes one session through a hand-built file before any
pipeline sits behind it.

- [x] Add `tippecanoe` to `just prerequisites` and to the README's list of what it installs.
- [x] Add the `mlt` CLI to `just prerequisites` and to the README's list of what it installs.
- [x] Install `tippecanoe` and the `mlt` CLI outside the sandbox with `just prerequisites`. The
      user runs this.
- [x] Vendor `@maplibre/mlt` and decode one MLT tile with it in a page that has no bundler. Do this
      first, as the remaining phase 1 tasks depend on it. Decoded under Node as an ES module with
      no bundler. A browser first imports it when `kiosk.js` does.
- [x] Write one session from `sessions.json` as a GeoJSON Text Sequence of sample points, by hand
      with `jq`, into the scratchpad. This is throwaway.
- [x] Build a throwaway MVT file from it with `tippecanoe`, with the samples at a single zoom level
      and their values as attributes. Convert it to `map.pmtiles` with `mlt convert`.
- [x] Add a server test in `crates/server/tests/server` asserting that a range request for a
      `.pmtiles` file returns 206 with no `Content-Encoding`. Make it pass in
      `crates/server/src/lib.rs`. It passed unchanged: `CompressionLayer` leaves a response with
      `Content-Range` uncompressed.
- [x] Add MapLibre GL JS and `pmtiles` to `just vendor`, and run it.
- [x] In `kiosk.js`, read the throwaway file's samples for that session, deduplicate them, sort
      them, and replay them through `lookout-predictor` in place of `sessions.json`. The
      throwaway holds all five sessions of the sessions version being served.
- [x] Check that the replayed samples equal that session's samples in `sessions.json`, field by
      field.
- [x] Measure the file's size, and the time the kiosk takes to read the session's samples, at
      zoom levels 6, 8, and 10. Choose the `samples` zoom level from those.

#### Phase 2: the map on a wide screen

- [x] In `kiosk/index.html` and `kiosk.js`, create a MapLibre map over the full viewport when
      `(min-width: 768px)` matches, with `lookout-predictor` placed over it at the top right. Give
      its source `"encoding": "mlt"`. MapLibre loads only when the map opens, and `journeys.js`
      finds tiles through d3-geo's Mercator projection, which the predictor already loads.
- [x] Draw the session's route from its samples as a line, and fit the map to its bounding box at
      each journey's `begin`. The line comes from the `route` layer, filtered to the session, with
      routes kept to zoom level 12 so a fitted journey is not drawn from zoom level 6 geometry.
- [x] Move a dot along the route from a GeoJSON source, updated as each sample is sent.
- [x] Check below 768 px that the page creates no map and lays out as it does now, and that
      crossing the width while open moves between the two layouts.

#### Phase 3: the rail and water the crossings name

What the crossings name, measured before the map's zoom levels are chosen. The map reads these
features live, so silver gains no datasets.

- [x] Derive silver in this worktree with `just init`, outside the sandbox. Its silver predates
      `water_crossing_v2`, and the crossings notebook needs DuckDB's spatial extension, which the
      sandbox cannot install. The user runs this.
- [x] Count the distinct rail and water ids `water_crossing_v2` names per country, and their WKB
      bytes in the extract, before choosing the map's zoom levels.
- [-] Choose the crate that derives `rail_v1` and `water_v1`, and record the choice under
      Decisions. Moot: the map reads the features live.
- [-] Add `RailRowV1` and `WaterRowV1` to `medallion-model`, with their declaration snapshots.
      Added, then removed once the map read the features live.
- [-] Add a binary that reads `water_crossing_v2`, takes each named rail segment and water feature
      from the crossing's extract, and writes `rail_v1` and `water_v1` through `write_table`.
      Moot: `pack_map` does the join.
- [-] Test that every rail and water id `water_crossing_v2` names appears exactly once in
      `rail_v1` or `water_v1`. Moved to phase 4, against `pack_map`'s output.
- [-] Add a `silver-crossed-features` recipe, and run it from `silver-crossings` after
      `silver-water-crossings`. Moot: no silver datasets.
- [-] Add `rail_v1` and `water_v1` to the derivation graph in `docs/architecture.md`, and to
      silver's datasets in `docs/medallion.md`. Moot: the wrap-up's doc tasks cover the live read.

#### Phase 4: the `map` artefact from the store

- [x] Create the `map` crate with a `pack_map` binary, with arguments shaped like
      `pack_sessions`: `MedallionArgs`, `--min-crossings`, `--max-sessions`, `--output`, and
      `--version-file`.
- [x] Write the chosen sessions' samples, as points, and their routes, as lines, as GeoJSON Text
      Sequences, with each feature's zoom range set in its `tippecanoe` member.
- [x] For every country, join `water_crossing_v2` to the bronze extract's segments and water on
      the Overture id and `extract_id`. Write the result as `rail` and `water`, from zoom level 7.
- [x] Test that `pack_map` writes every rail and water id `water_crossing_v2` names exactly once,
      and fails when an extract lacks one.
- [x] Measure the artefact's size against the `rail` and `water` maximum zoom level, and choose
      the level from those sizes.
- [x] Run `tippecanoe` from `pack_map` into a temporary MVT file, and `mlt convert` on it into
      `gold/artifact=map/version=<run>/map.pmtiles`.
- [x] Add `map.version`, and the Justfile recipes `gold-pack-map` and `kiosk-map`, and make
      `serve-web` depend on `kiosk-map`.
- [x] Copy the map artefact in the Dockerfile from a `MAP_VERSION` build argument, and allow
      `gold/artifact=map` in `.dockerignore`.
- [x] Copy the packed artefact over the throwaway `map.pmtiles` in `static`.
- [x] Style `rail` and `water` in the kiosk's MapLibre style.
- [x] Pack the map artefact, commit its version, and check the kiosk on a deploy at both widths.

#### Wrap-up

- [-] Name the `write_table` workaround, and the writer that replaces it, in this slice's entry in
      `completed-slices.md`, so that a branch rebasing onto this one finds the debt. Moot: no
      workaround remains.
- [x] Remove `sessions.version`, `gold-pack-sessions`, `kiosk-sessions`, the Dockerfile's
      `SESSIONS_VERSION`, and the `sessions` line in `.dockerignore`. Do this only after the
      kiosk replays from the map on a deploy.
- [x] Decide whether `pack_sessions` and the committed `gold/artifact=sessions` versions go too.
      The rerun runner and the notebooks do not read them. Both went.
- [ ] Update `docs/web.md` on what `/kiosk` feeds the element and the map it draws.
- [x] Update the derivation graph and the gold consumers in `docs/architecture.md`.
- [x] Update the gold exceptions in `docs/medallion.md`, naming the map artefact and `map.version`.
- [ ] State in `docs/medallion.md`'s gold section that an artefact may read an upstream reference
      extract in bronze, such as Overture's, selecting rows by the ids a silver dataset names. The
      allowance covers external reference data only. Observations in bronze still reach gold
      through silver.
- [ ] Write `crates/map/README.md`.
- [ ] Record under Observations what the spike measured: the file size, the read time, and the
      zoom levels chosen. This is what the App slice needs to know.
- [ ] Run `just test-no-docker` and `cargo clippy --workspace`, and ask the user to run `just test`.
