# Current Slice: getting a second source of position data from Motis

### Target

Whilst I am travelling on a train I'd like to get a secondary (non-GPS) source of data by periodically polling a 
local [Motis](https://github.com/motis-project/motis/) instance running with data for Germany. 

#### Info

I have a local motis server installation (see tools/motis-server/Justfile) running with data for Germany. It is listening on http://localhost:8080.

### Straw Man Architecture

The idea would be to do something like the following in a continuous loop:
1. Poll the redis queue for recently logged gps positions, covering the past N minutes (ignore anything old)
2. Maintain a local set which contains all positions seen over past 30 mins
3. Building a bounding-box which covers the area of these GPS positions, expanded with a buffer; let's say double the size
4. Query motis for this bounding box to find all train positions in this region
5. Log this data to a local sqlite `motis` db, with duplication allowed

The intent is then to take this raw data in the db, and ingest it alongside the existing gps data in the `lookout` db to produce a visualisation of train positions over the same time period as the gps traces being visualised.

### Motis API (researched against the running v2.10.2 server)

Endpoint: `GET /api/v1/map/trips` (canonical `/api/v6/map/trips`; the server is
version-tolerant). Required query params: `zoom` (number — filters by mode: low zoom =
long-distance only, high zoom adds subway/tram/bus), `min` = `"lat,lon"`, `max` =
`"lat,lon"`, `startTime`/`endTime` (RFC3339). Optional `precision` (polyline precision,
default 5). Empirically `min` is the SW corner (`min_lat,min_lon`) and `max` the NE
corner (`max_lat,max_lon`) — the spec's "lower-right/upper-left" wording is misleading;
a plain min/max box returns correct data.

Response: a JSON array of `TripSegment`, one stop-to-stop leg each: `trips[]`
(`tripId`, `routeShortName`/`displayName`), `mode` (`REGIONAL_RAIL`/`SUBWAY`/`TRAM`/
`BUS`/`HIGHSPEED_RAIL`/…), `routeColor?`, `distance` (m), `realTime` (bool), `from`/`to`
`Place` (`name`, `stopId`, `lat`, `lon`, `departure`/`arrival`), the segment's
`departure`/`arrival`/`scheduled*` times, and `polyline` (Google-encoded, precision 5).

**Facts that shape the design:**
- **`map/trips` gives interpolated, not raw-GPS, positions.** A `TripSegment` is trip
  geometry ("trip X runs A→B along this polyline, departing 10:20, arriving 10:28"). A
  train's position *at time T* is obtained by **interpolating** along the segment whose
  `[departure, arrival]` spans T. That interpolation is the "map to lat/lon positions".
  There is no vehicle-positions endpoint in the API.
- **Realtime = delay-corrected times, and it must be enabled.** With a GTFS-RT feed
  loaded, `map/trips` returns `realTime: true` segments whose `departure`/`arrival` carry
  actual delays (while `scheduledDeparture`/`scheduledArrival` stay as the plan), so the
  interpolated position reflects current reality. The running server currently has **no**
  RT feed (verified: all ~1800 segments `realTime: false`, zero delay, no `rt:` in
  `config.yml`), so this slice **enables** one. Note: the free German feed
  (gtfs.de) carries **TripUpdates + ServiceAlerts only — no VehiclePositions** — so the
  best available is realtime-corrected interpolation, nationwide, not a raw GPS dot.
- **Polylines are stop-to-stop straight lines** (verified: all ~1800 Frankfurt segments
  decode to ≤2 points) because **the gtfs.de free feed ships no `shapes.txt`** (absent from
  the zip; no `shape_id` in `trips.txt`), so Motis only has stop coordinates. Motis does
  *not* auto-generate shapes from OSM today (it's on their TODO; `with_shapes: true` +
  loaded OSM still yields straight lines). Fix: generate `shapes.txt` from OSM with
  `pfaedle` (rail-only) and re-import — then interpolation follows real track geometry.
  Until then, interpolation cuts corners between stops (degraded, not broken).

**Client decision:** depend on the maintained `motis-openapi-progenitor` crate (0.4.0,
progenitor-generated from this spec, reqwest 0.12, exposes `.trips()` and
`types::TripSegment`) rather than hand-writing or generating in-repo. Polyline decoding
uses the `polyline` crate.

A real 4-segment, mode-varied fixture is captured for the dedup/decoding tests.

### Tasks

Two halves. **Capture:** a new `motis` crate (lib + a `motis_poll` binary) runs the
straw-man loop — non-destructively read recent GPS off the redis queue, keep a rolling
window of them, query the local Motis server for trips in a buffered bounding box, and
append the returned segments to a raw, duplication-allowed `motis` SQLite db.
**Visualise:** a Rust ingest step dedups that raw log and decodes it to a derived
`train_segment` table in the `lookout` db, and `visualise` interpolates + logs the
moving trains to rerun alongside the GPS traces over the same window. Follows the
existing store pattern (`recorder::store` / `transport::store`), the "Rust writes
derived tables, Python reads" split, and reuses `telemetry` for redis. TDD, keeping the
code compiling at every step.

- [x] **Enable realtime in the Motis server.** Add the gtfs.de free RT feed to the
  `germanygtfs` dataset in `tools/motis-server/motis_server/config.yml` (and mirror into
  the Justfile's `motis config` step so it survives a re-config):
  ```yaml
  rt:
    - url: https://realtime.gtfs.de/realtime-free.pb
      protocol: gtfsrt
  ```
  Restart `motis server` (no re-import needed — RT is applied at runtime, polled every
  `update_interval` = 60s). Verify: `map/trips` now returns some segments with
  `realTime: true` and `departure != scheduledDeparture`.
  > **Done differently:** `motis server` reads `data/config.yml` (the expanded config
  > `import` writes), *not* the top-level `config.yml`. So the Justfile got a `enable_rt`
  > recipe that `yq`-patches `.timetable.datasets.germanygtfs.rt` into `data/config.yml`
  > (idempotent `=`; motis has no config-override/env mechanism, `yq` beats an awk hack),
  > plus a `prerequisites` recipe (`brew install yq`) it depends on, and `motis_setup`
  > runs it after `import`. Verified: 201/221 segments `realTime: true`, 118 delay-corrected.
- [x] **Scaffold the `motis` crate + workspace wiring.** New `crates/motis` (lib + a
  `src/bin/motis_poll.rs`), picked up by the existing `crates/*` workspace glob. Add
  `motis-openapi-progenitor`, `polyline`, and (if needed for RFC3339 window formatting)
  `chrono` to `[workspace.dependencies]`. Empty lib compiles and `just test-no-docker`
  passes.
- [x] **Promote `BBox` to `shared`.** Move `transport::groups::BBox` into `shared` (a
  pure lat/lon data type — keep the "double it" buffer policy out of it) and update
  `transport`'s call sites to import from `shared` directly (no re-export shim). Both
  crates and their tests still pass.
- [x] **`window` module — rolling GPS position set + buffered bbox.** A `PositionWindow`
  that ingests `(t, lat, lon)`, prunes entries older than a configurable age (default
  30 min) relative to `now`, exposes the tight `BBox` of what it holds (`Option`, `None`
  when empty), and a buffered box that doubles each dimension about its centre. Pure
  logic → unit + `proptest` invariant tests (buffered box contains the tight box; pruning
  is monotonic).
- [x] **`client` module — thin wrapper over `motis-openapi-progenitor`.** A `MotisClient`
  (base URL, default `http://localhost:8080`) with
  `trips_in_bbox(&BBox, window, zoom) -> Result<Vec<TripSegment>, MotisError>` that maps
  our `BBox` → `min`/`max` `"lat,lon"` strings and the window → RFC3339 `startTime`/
  `endTime`, calls `.trips()`, and surfaces failures as a `thiserror` `MotisError`. Test
  the bbox/window → params mapping; a `#[ignore]` live smoke test against `localhost:8080`.
  > **Notes / gotchas found driving it against the live server:**
  > - Test is an `end_to_end`-named test (skipped by default/no-docker, run via
  >   `just end_to_end_test`), not `#[ignore]`, per the repo's nextest convention.
  > - **Default is `http://127.0.0.1:8080`, not `localhost`**: Motis binds IPv4
  >   `0.0.0.0`, but `localhost` resolves to IPv6 `::1` first, which never connects.
  > - **Whole-second timestamps required**: progenitor serialises `DateTime<Utc>` with
  >   micros, and Motis `map/trips` mis-parses fractional-second bounds (result swings
  >   between empty and huge). The client truncates the window to whole seconds.
- [x] **`store` module — append-only raw `motis` SQLite db.** Schema-on-open like the
  other stores, but **duplication allowed** (no unique key): autoincrement rowid,
  `captured_at` (poll time), plus the segment fields we keep — `trip_id`, `route_name`,
  `mode`, `route_color`, `from`/`to` stop id + lat/lon, `departure`/`arrival` and
  `scheduled_departure`/`scheduled_arrival` (epoch ms — keep both so delay is
  recoverable), `realtime` (bool), and the raw `polyline` string. `insert` always appends
  — a test proves the same segment inserted twice yields two rows. In-memory test.
- [x] **`motis_poll` binary — the continuous loop.** Wire it together: `clap` args for
  poll interval, window age, recent-lookback minutes, `zoom`, `--motis-url`
  (default `http://localhost:8080`), `--db` (default `data/motis.sqlite`); redis URL from
  `LOOKOUT_REDIS_URL` like the recorder. Each tick: `latest_samples` → filter to GPS
  within the recent window → update the `PositionWindow` → buffered bbox → query Motis for
  a short time window around now → append segments to the store, with structured
  `tracing`. Ctrl-C stops cleanly.
- [x] **Justfile `poll-motis` recipe.** Wrap the binary in `op run` for the redis URL
  (mirroring `record`).
- [x] **Verify the capture half.** With the Motis server and real redis running, run the
  loop and confirm segment rows land in `data/motis.sqlite`. Use `/verify`.
- [ ] **Ingest — dedup + decode into the `lookout` db.** A `motis_ingest` binary (in the
  `motis` crate) that reads the raw, duplication-allowed `motis.sqlite`, **dedups**
  segments on `(trip_id, from_stop_id, departure)` (the same scheduled leg re-seen across
  overlapping polls collapses to one row — prefer the newest `captured_at`'s realtime
  values), **decodes** each `polyline` (the `polyline` crate) to a lat/lon `LineString`
  stored as WKB, and writes a derived `train_segment` table into `data/lookout.sqlite`
  (idempotent `INSERT OR IGNORE`, mirroring `enrich`; WKB geom like the `transport`
  table). Keeps `trip_id`/`route_name`/`mode`/`route_color`/`realtime` and the
  realtime-corrected `departure`/`arrival`. Add a Justfile `ingest-motis` recipe
  (mirroring `enrich`). Unit test dedup + polyline decode over the captured fixture rows.
- [ ] **Visualise the trains.** Extend `visualise/main.py`: read `train_segment`
  (time-windowed by `--since`), and for each timeline tick **interpolate** each train's
  position along its WKB line by `(t − departure)/(arrival − departure)` using the
  realtime-corrected times (shapely `line.interpolate(frac, normalized=True)` — shapely is
  already a dep) → a per-trip `GeoPoints` moving dot, coloured by `mode`/`routeColor`;
  optionally the static per-trip route line. Fold into the map blueprint so trains and GPS
  traces share the same view and window. Add Python tests in `visualise/tests`. Absent
  table → no-op (like `transport`).
- [ ] **Verify the full pipeline.** `/verify` the whole chain end to end: poll → ingest →
  visualise, confirming the `.rrd` shows trains moving (using realtime-corrected timing)
  over the same window as the GPS traces (`rerun rrd verify` passes).
- [ ] **(Quality improvement) Generate real track geometry with pfaedle.** The gtfs.de
  feed has no `shapes.txt`, so `map/trips` polylines are straight stop-to-stop lines and
  interpolation cuts corners. In `tools/motis-server`, add a Justfile step that runs
  `pfaedle` (rail-only) over the OSM + GTFS to produce shapes and repackage the feed, e.g.
  `pfaedle -x geo_data/germany.osm.pbf -m rail -X geo_data/filtered.osm.pbf geo_data/germany_gtfs.zip`,
  then zip `gtfs-out/` back over `germany_gtfs.zip` and re-run `motis import` + restart.
  Heavy one-off run. Verify: `map/trips` polylines now decode to many points (curved
  track), not 2, so the trains follow the rails. Independent of the pipeline above — a
  drop-in geometry upgrade the visualisation picks up automatically on the next ingest.

Note: positions are realtime-corrected *interpolation* (a second, non-GPS reference),
not raw vehicle GPS — the best the gtfs.de TripUpdates feed allows nationwide. Consider
filtering to rail-ish modes (drop `BUS`) in the poll or ingest step so the visualisation
is trains, not all transit.

## Pending refactors

* [ ] switch geo types (e.g. BBox) to instead use shared types from external Rust geo libraries