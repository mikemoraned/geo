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

### Tasks

Two halves. **Capture:** a new `motis` crate (lib + a `motis_poll` binary) runs the
straw-man loop — non-destructively read recent GPS off the redis queue, keep a rolling
window of them, query the local Motis server for trains in a buffered bounding box, and
append the results to a raw, duplication-allowed `motis` SQLite db. **Visualise:** a
Rust ingest step dedups that raw log and maps it to a derived `train_position` table in
the `lookout` db, and `visualise` logs the moving trains to rerun alongside the GPS
traces over the same window. Follows the existing store pattern (`recorder::store` /
`transport::store`), the "Rust writes derived tables, Python reads" split, and reuses
`telemetry` for redis. TDD, keeping the code compiling at every step.

- [ ] **Scaffold the `motis` crate + workspace wiring.** New `crates/motis` (lib + a
  `src/bin/motis_poll.rs`), picked up by the existing `crates/*` workspace glob. Add an
  HTTP client dep (`reqwest`, rustls-tls, `json`, `default-features = false` — to match
  the existing rustls stack and avoid openssl) to `[workspace.dependencies]`. Empty lib
  compiles and `just test-no-docker` passes.
- [ ] **Promote `BBox` to `shared`.** Move `transport::groups::BBox` into `shared` (a
  pure lat/lon data type — keep the "double it" buffer policy out of it) and update
  `transport`'s call sites to import from `shared` directly (no re-export shim). Both
  crates and their tests still pass.
- [ ] **`window` module — rolling GPS position set + buffered bbox.** A `PositionWindow`
  that ingests `(t, lat, lon)`, prunes entries older than a configurable age (default
  30 min) relative to `now`, exposes the tight `BBox` of what it holds (`Option`, `None`
  when empty), and a buffered box that doubles each dimension about its centre. Pure
  logic → unit + `proptest` invariant tests (buffered box contains the tight box; pruning
  is monotonic).
- [ ] **`store` module — append-only `motis` SQLite db.** Schema-on-open like the other
  stores, but **duplication allowed**: an autoincrement rowid, a `captured_at` (poll
  time), plus the train fields (trip id, route/line name, lat/lon, whatever the Motis
  response carries). `insert` always appends — a test proves inserting the same position
  twice yields two rows. In-memory test.
- [ ] **`client` module — query Motis for trains in a bbox.** First verify the endpoint
  and response shape against the running server (`localhost:8080`; likely
  `GET /api/v1/map/trips` with min/max lat-lon) and capture a real response as a test
  fixture. Then a typed `TrainPosition` + `thiserror` `MotisError` and
  `MotisClient::trains_in_bbox(&BBox)`. Main test: parse the captured fixture into typed
  positions.
- [ ] **`motis_poll` binary — the continuous loop.** Wire it together: `clap` args for
  poll interval, window age, recent-lookback minutes, `--motis-url`
  (default `http://localhost:8080`), `--db` (default `data/motis.sqlite`); redis URL
  from `LOOKOUT_REDIS_URL` like the recorder. Each tick: `latest_samples` → filter to
  GPS within the recent window → update the `PositionWindow` → buffered bbox → query
  Motis → append to the store, with structured `tracing`. Ctrl-C stops cleanly.
- [ ] **Justfile `poll-motis` recipe.** Wrap the binary in `op run` for the redis URL
  (mirroring `record`).
- [ ] **Verify the capture half.** With the Motis server and real redis running, run the
  loop and confirm train-position rows land in `data/motis.sqlite`. Use `/verify`.
- [ ] **Ingest — dedup + map into the `lookout` db.** A `motis_ingest` binary (in the
  `motis` crate) that reads the raw, duplication-allowed `motis.sqlite`, **dedups** each
  train observation (e.g. on `(trip_id, observed_at)` — the same trip re-seen across
  overlapping polls collapses to one row) and **maps** it to a lat/lon position, writing
  a derived `train_position` table into `data/lookout.sqlite` (idempotent
  `INSERT OR IGNORE`, mirroring `enrich`). Any polyline/coordinate decoding the Motis
  form needs lives here. Add a Justfile `ingest-motis` recipe (mirroring `enrich`). Unit
  test the dedup + mapping over captured raw rows.
- [ ] **Visualise the trains.** Extend `visualise/main.py`: read `train_position`
  (time-windowed by `--since` like `gps`), and log each train as a per-timestamp
  `GeoPoints` that follows the timeline (a moving dot, like the GPS cursor), keyed per
  trip and coloured per line/route, plus optionally a per-trip track polyline. Fold into
  the map blueprint so trains and GPS traces share the same view and window. Add Python
  tests in `visualise/tests`. Absent table → no-op (like `transport`).
- [ ] **Verify the full pipeline.** `/verify` the whole chain end to end: poll → ingest →
  visualise, confirming the `.rrd` shows trains moving over the same window as the GPS
  traces (`rerun rrd verify` passes).
