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

- `rail` and `water`: Overture's rail and water within a corridor around each replayed route,
  drawn as the base map.
- `route`: each replayed session as one line, drawn on the base map.
- `samples`: each replayed sample as a point, at one zoom level only, read by the kiosk rather
  than drawn.

The file's metadata lists the sessions it holds: the id, the crossings count, the bounding box,
and the sample count.

The artefact holds no land, roads, or place labels, and no crossings. The browser still fetches
`crossings.json` for the predictor.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-11-15.

- **The base map is Overture rail and water, clipped to a corridor around the replayed routes.**
  Bronze already holds both countries' extracts, and the App will later want the same two layers
  on the M5. The alternatives were a Protomaps planet extract (larger, and none of it our own
  data) and hosted tiles (these test nothing about embedding).
- **The corridor reaches as far from the route as any predictor looks.** That is 5 km today,
  `predictor::crow_flies::DEFAULT_RADIUS`, and `pack_map` reads it from the `predictor` crate. The
  App will show nothing beyond that distance, so rail and water further out would cost bytes
  nobody sees. The App does not show a map yet, so the kiosk does not test this width.
- **The replay reads its samples from the PMTiles, as the Straw Man proposes.** The kiosk stops
  fetching `sessions.json`. The alternative was to keep `sessions.json` for the replay and use the
  tiles for display only.
- **A sample carries its full values as attributes.** These are the session, its position in the
  session, `t`, `lat`, `lon`, `alt`, `acc`, `speed`, and `heading`. A vector tile quantises its
  geometry to the tile grid, so position read back from the geometry loses precision. The
  attributes keep the six decimal places gold writes.
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
- **`tippecanoe` tiles, `mlt convert` encodes, and Rust writes the input.** A new crate, `map`,
  reads the chosen sessions and the Overture rail and water through medallion's `Query`, and
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

- `sessions.version`, `gold-pack-sessions`, `kiosk-sessions`, and the Dockerfile's
  `SESSIONS_VERSION` lose their only consumer once the kiosk reads the map.
- Building tiles needs `tippecanoe` and the `mlt` CLI on the machine that packs gold. The deploy
  needs neither, as it copies the committed artefact.

#### Rejected / deferred

- **Land, roads, and labels underneath.** These would come back if the bare rail and water fail to
  place a route. A Protomaps extract would then go under the Overture layers.
- **Reading samples through MapLibre's `querySourceFeatures`.** It returns only the features in the
  tiles MapLibre has loaded for display, clipped to them, so a journey would read differently at
  each zoom level.

#### Open questions

- The zoom range of `rail` and `water`, and the `samples` zoom level. Sizes measured during the
  work settle both, as well as whether the artefact stays small enough to commit beside the
  other gold artefacts.

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
- **`mlt convert` writes a `.pmtiles` input only to a `.pmtiles` or `.mbtiles` output.** Loose
  `.mlt` tiles come from converting tippecanoe's directory output (`-e`).

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
- [ ] In `kiosk.js`, read the throwaway file's samples for that session, deduplicate them, sort
      them, and replay them through `lookout-predictor` in place of `sessions.json`.
- [ ] Check that the replayed samples equal that session's samples in `sessions.json`, field by
      field.
- [ ] Measure the file's size, and the time the kiosk takes to read the session's samples, at
      zoom levels 6, 8, and 10. Choose the `samples` zoom level from those.

#### Phase 2: the map on a wide screen

- [ ] In `kiosk/index.html` and `kiosk.js`, create a MapLibre map over the full viewport when
      `(min-width: 768px)` matches, with `lookout-predictor` placed over it at the top right. Give
      its source `"encoding": "mlt"`.
- [ ] Draw the session's route from its samples as a line, and fit the map to its bounding box at
      each journey's `begin`.
- [ ] Move a dot along the route from a GeoJSON source, updated as each sample is sent.
- [ ] Check below 768 px that the page creates no map and lays out as it does now, and that
      crossing the width while open moves between the two layouts.

#### Phase 3: the `map` artefact from the store

- [ ] Create the `map` crate with a `pack_map` binary, with arguments shaped like
      `pack_sessions`: `MedallionArgs`, `--min-crossings`, `--max-sessions`, `--output`, and
      `--version-file`.
- [ ] Write the chosen sessions' samples, as points, and their routes, as lines, as GeoJSON Text
      Sequences, with each feature's zoom range set in its `tippecanoe` member.
- [ ] Query Overture rail and water within `DEFAULT_RADIUS` of each route, from the newest
      extract for its country, and write them as `rail` and `water`.
- [ ] Measure the artefact's size against the `rail` and `water` maximum zoom level, and choose
      the level from those sizes.
- [ ] Run `tippecanoe` from `pack_map` into a temporary MVT file, with the session list in its
      metadata. Run `mlt convert` on it into `gold/artifact=map/version=<run>/map.pmtiles`, and
      check that the metadata survives the conversion.
- [ ] Add `map.version`, and the Justfile recipes `gold-pack-map` and `kiosk-map`, and make
      `serve-web` depend on `kiosk-map`.
- [ ] Copy the map artefact in the Dockerfile from a `MAP_VERSION` build argument, and allow
      `gold/artifact=map` in `.dockerignore`.
- [ ] Point `kiosk.js` at `/map.pmtiles`, read the session list from the metadata, and delete the
      throwaway file.
- [ ] Style `rail` and `water` in the kiosk's MapLibre style.
- [ ] Pack the map artefact, commit its version, and check the kiosk on a deploy at both widths.

#### Wrap-up

- [ ] Remove `sessions.version`, `gold-pack-sessions`, `kiosk-sessions`, the Dockerfile's
      `SESSIONS_VERSION`, and the `sessions` line in `.dockerignore`. Do this only after the
      kiosk replays from the map on a deploy.
- [ ] Decide whether `pack_sessions` and the committed `gold/artifact=sessions` versions go too.
      The rerun runner and the notebooks do not read them.
- [ ] Update `docs/web.md` on what `/kiosk` feeds the element and the map it draws.
- [ ] Update the derivation graph and the gold consumers in `docs/architecture.md`.
- [ ] Update the gold exceptions in `docs/medallion.md`, naming the map artefact and `map.version`.
- [ ] Write `crates/map/README.md`.
- [ ] Record under Observations what the spike measured: the file size, the read time, and the
      zoom levels chosen. This is what the App slice needs to know.
- [ ] Run `just test-no-docker` and `cargo clippy --workspace`, and ask the user to run `just test`.
