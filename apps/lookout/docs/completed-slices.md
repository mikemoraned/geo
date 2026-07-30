# Completed Slices

Append-only history of finished slices. Never edit existing entries; `/complete-slice`
adds a new condensed summary here when a slice is done.

## bootstrap getting sensor data and saving it in rerun.io format

Aimed to stand up a minimal localhost-only pipeline: a Rust web server serving a page
that reads laptop accelerometer data, streams it over a websocket, and persists it in
rerun.io format. **Abandoned part-way on a learned constraint**, but the transport half
was built and verified.

- Scaffolded a bobby-inspired `crates/` cargo workspace with a new axum `server` crate
  serving static assets on localhost, plus a `/ws` websocket endpoint that receives and
  logs JSON accel samples. Added a vanilla `index.html` + `app.js` front-end doing the
  `DeviceMotionEvent` permission flow, listener, and websocket send.
- Verified the browser→server websocket transport works end to end (socket connects,
  server logs samples). The rerun persistence half and the planned crux/ports-and-adapters
  refactor were not reached.
- **Key constraint discovered:** the dev machine (Apple Silicon M3 MacBook Air) has no
  accelerometer — Apple Silicon dropped the Sudden Motion Sensor, so `DeviceMotionEvent`
  never fires on the laptop. A localhost-only, laptop-only setup can validate transport
  and persistence but cannot source real motion data.
- **Implication:** future slices needing real motion must use an external source —
  AirPods (`CMHeadphoneMotionManager`), an IMU game controller (native HID), or an
  iPhone/iPad over HTTPS (needs a cert or tunnel, the LAN work this slice deferred).
  Recorded in `target.md` under Learned Constraints.

## mike is on a train getting data

Built the full pipeline end to end: an iPhone samples GPS + accelerometer over the
train journey, streams them to a fly.io-hosted server, and the data is drained locally
into SQLite and visualised in rerun. The laptop-has-no-accelerometer constraint was
resolved by making the phone the sensor and serving over fly.io's public HTTPS (secure
context for Geolocation / DeviceMotion, `wss://` websocket — no cert/LAN work needed).

- **Frontend**: vanilla page that persists a `crypto.randomUUID()` device id in a cookie,
  samples `devicemotion` + `geolocation.watchPosition` throttled to a fixed interval, and
  sends timestamped JSON samples over the websocket with a best-effort in-memory outbox
  flushed on reconnect (not persisted, drops oldest on overflow).
- **Transport**: server `LPUSH`es samples onto an upstash redis list; pushing goes through
  a `SampleSink` port (`RedisSink` for prod) so `/ws` is covered by a Docker-free integ
  test. Redis is optional (log-only when unset) but fails loud when configured yet
  unreachable. Added a `/version` endpoint + build-git-hash startup log for debugging.
- **New crates**: `shared` (the `Sample` model), `telemetry` (queue connect/drain, returns
  a lossless `RawSample`), and a `recorder` cli. Deleted the rerun-as-archive plan.
- **Recorder cli**: `view-latest` (non-destructive `LRANGE`) and `drain` (destructive
  `BRPOP`) modes writing to SQLite — a lossless `raw(md5,json)` table plus per-sensor
  `accel`/`gps` tables, all `INSERT OR IGNORE`. Per-sensor tables are a rebuildable
  derivation of raw.
- **Visualise**: a Python `uv` project (rerun-sdk 0.34) converting SQLite → rrd, selecting
  by `--since <Nd>` / `--devices`. Blueprint pairs a map view with per-device accel
  time-series. GPS logged as a static `GeoLineStrings` track plus per-fix `GeoPoints` with
  accuracy radii; accel logged as one `send_columns` entity with a derived `|a|` magnitude
  series (the one orientation-invariant signal in gravity-dominated data).
- **Secrets**: bobby's 1Password pattern — checked-in `deploy/*.env` hold only `op://`
  references; local runs wrap in `op run`, fly deploy pushes resolved values via
  `fly secrets set`. No secret values committed.
- Proved on a real journey: 3 devices, 292 accel + 163 gps samples drained to
  `data/lookout.sqlite`, converted to `.rrd` (`rerun rrd verify` passes), and confirmed
  visible in the rerun viewer.

## improve train-based recording accuracy / reliability

Hardened the capture pipeline so a train journey yields data that both arrives reliably
and means something, targeting iOS/macOS Safari only. Split into a wire-model refactor,
capture-survival work on the frontend, and data-quality additions through to the rerun
views. The pre-journey dry run was deliberately skipped ("risk it on the day").

- **Versioned wire model**: replaced the flat `Sample` with a two-level `Message` enum
  (`Version0` / `Version1`), each wrapping an inner message set. `v` is carried on the
  wire and defaults to 0 when absent, so historical unversioned payloads in `raw` still
  parse. The `shared` crate was reorganised into `message` / `sensor` / `session` modules.
- **Session metadata**: a new `StartSession` message (v1) lets a device announce its
  class (iPhone / iPad / laptop, classified client-side from `navigator`) at record time.
  It's interpreted into a new `device` table keyed on `device_id`; sensor rows seed a
  minimal `unknown` placeholder so every reading has a device row to join to.
- **Capture survival (frontend)**: screen wake lock re-acquired on visibility change
  (surfaced in the UI), an outbox persisted to `localStorage` and re-flushed on startup,
  and server acks so a sample leaves the outbox only once confirmed — a reload or
  mid-flush drop re-sends the un-acked tail instead of losing it.
- **Data quality**: GPS gained Doppler `speed` / `heading` (nulls preserved) and now
  stamps the fix's own timestamp rather than send time; accel is aggregated over the
  window into `rms` / `peak` / `n` (gravity-removed), keeping one raw x/y/z for tilt.
- **Persistence / server**: the queue item became a `RawSample` envelope carrying a
  server-stamped `received_at` beside the verbatim payload (md5 contract intact). The
  archive migrates older schemas on open via `ALTER TABLE ADD COLUMN`, and the recorder
  requeues a sample on archive failure rather than draining past it and losing it.
- **Views**: the rerun track is now coloured by speed (viridis ramp), `rms`/`peak` show
  ride quality superseding the old `|a|` magnitude, and `n` plots as a capture-health
  signal exposing windows where the page was suspended.

## visualise transport geo data for regions

Overlaid the Overture rail network onto the device tracks in rerun, to see where journeys
correspond to transport segments. A new `enrich` CLI derives per-`(device, UTC day)`
bounding boxes from the archive's gps fixes, fetches the intersecting Overture rail data
live from public S3, and persists it into the same SQLite archive; the Python visualiser
then logs it as a static map backdrop.

- **New `transport` crate** with an `enrich` binary. Fetches Overture `theme=transportation`
  GeoParquet anonymously from S3 via **SedonaDB** (a Rust-native DataFusion/GeoArrow engine,
  a git dep) — chosen over duckdb to grow non-trivial spatial work in-process later.
- **Spatial pruning is essential**: filtering with `ST_Intersects` against a single
  `MULTIPOLYGON` of all bbox envelopes lets SedonaDB prune GeoParquet row groups by their
  bbox covering — ~1m vs ~13min for a numeric bbox filter that barely prunes. Geometry is
  read out as WKB via `ST_AsBinary`.
- **Rail only**: keep `subtype = 'rail'` segments and the connectors they reference (ids
  `UNNEST`ed from the segments), excluding `tram`-class rail (and its connectors) via a
  shared `EXCLUDED_CLASSES` predicate that still keeps null-class rows.
- **SQLite geo storage**: geometry as a WKB blob plus flattened bbox columns and an R\*Tree
  virtual table keyed on rowid (for later "within distance of a sample" queries). Idempotent
  `INSERT OR IGNORE` on the Overture GERS id, following the `recorder::store` pattern.
- **Visualise**: the Python converter reads the `transport` table, logs rail segments as
  static `GeoLineStrings` coloured by rail class and connectors as `GeoPoints`, parsing WKB
  with shapely and flipping stored `lon lat` to rerun's `(lat, lon)`. A shared transport map
  pane joins the per-device tiles; an un-enriched archive still visualises.
- **`--near <degrees>` (hack)**: optionally restrict segments to those within a raw planar
  degrees distance of a gps fix — a rough cut, not true ground distance (which would need
  reprojecting to a metric CRS); caveat noted in the help and code.

## getting a second source of position data from Motis

Added a second, non-GPS reference for train positions by polling a local Motis server
for Germany and folding the results in alongside the GPS traces. The key structural fact:
Motis' `map/trips` gives trip geometry, not vehicle GPS — a train's position at time T is
*interpolated* along the segment spanning T. No German open feed carries VehiclePositions,
so realtime-corrected interpolation is the nationwide ceiling; the server was configured
with a GTFS-RT feed to make that interpolation delay-aware.

- **New `motis` crate** (lib + `motis_poll`, `motis_ingest` binaries), depending on the
  maintained `motis-openapi-progenitor` client and the `polyline` crate. A `client` wraps
  `.trips()` and `/trip`; a `window` module keeps a rolling GPS set and derives a buffered
  bounding box; a `store` appends raw segments to a duplication-allowed `motis.sqlite`.
- **Capture loop** (`motis_poll`): reads recent GPS off redis non-destructively, builds a
  buffered bbox, queries Motis, filters to rail modes, resolves each trip via `/trip` for
  agency + train number, and appends segments. **Ingest** (`motis_ingest`) dedups on the
  scheduled leg, decodes polylines to WKB linestrings, and writes a derived `train_segment`
  table into `lookout.sqlite`. **Visualise** interpolates each train along its line by
  realtime-corrected timing into moving, labelled, mode-coloured dots sharing the GPS view.
- **Moved the server dataset from gtfs.de free to DELFI** (a plain public URL swap, static
  then RT feed). This fixed the core data gap: DELFI carries correct `route_type`, so `mode`
  alone now separates ICE/IC from S-Bahn, retiring the agency-based classification hack; and
  train numbers arrive via `/trip`'s `trip_short_name`. RT ingest went to ~99.97% success.
- **`BBox` promoted to `shared`**; geo types reuse external crates (`geo` `BoundingRect`/
  `Scale`, `wkt`, `wkb`) rather than hand-rolled arithmetic.
- **Parked (not done): pfaedle rail track geometry.** DELFI's `shapes.txt` covers only
  bus/coach, so rail interpolation still cuts corners. The pfaedle tooling was built and
  produces correct curved rail, but importing its `shapes.txt` breaks GTFS-RT realtime; decided to give up on fixing this for now.
  Full write-up and resume steps live in `.claude/memory/motis-trips-api.md`.

## minimal version of water crossings

Built a dataset of where visible water bodies cross rail lines in Germany, entirely in marimo
notebooks (no new crates) under `apps/lookout/notebooks/water_crossings/`, using DuckDB's
`spatial` extension to read Overture GeoParquet directly, intersect rail against water in SQL,
and visualise with lonboard. Each notebook is a self-contained version building on the last;
outputs export to GeoParquet and to uncompressed native GeoArrow for kepler.gl.

- Progressed from a four-state extract to all of Germany; added deduplication to collapse the
  many redundant crossings down to roughly one per physical track × water body; and added a filter
  to drop crossings the train can't actually see (tunnels, non-running track).
- Built a small bbox test-case harness with a per-case viewer (linking to the OvertureMaps
  explorer) to validate counts against hand-checked truth — which caught a wrong assumption.
- **The durable learnings — where the real difficulty lay (deduplication is water-side + spatial,
  not rail-side; a 2D intersection isn't "visible water"; Overture's representation quirks) — are
  captured in `docs/target.md` under Learnings.**

## Spikes on Device Support

Showed that the pieces a live predictor would depend on can each run on an M5StickC PLUS2 with
an AT6668 GPS unit — enough to say it looks feasible, not that it works. Five standalone spikes
in `apps/lookout/spikes/m5/` build on each other: toolchain and flash, screen, Crux on device,
GPS in, BLE out. The end state shows time and lat/lon on the panel and publishes position over
BLE; no prediction logic exists yet. New crates on device: `esp-idf-svc`, `mipidsi` +
`embedded-graphics`, `nmea`, `esp32-nimble`, and `crux_core`.

- **Core and shell must be separate crates.** `esp-idf-sys`'s build script aborts on a host
  target, so anything depending on it can never be tested off-device — and host-testability was
  the whole reason for Crux. Each spike from 2 onward is an esp-free `core/` plus an esp-idf
  `shell/`, with the shell importing the core directly (no typegen or bridge; those are for
  non-Rust shells).
- **The shell only carries out effects.** The core owns what the screen says and, in spike 4,
  the BLE payload and the decision of when publishing is worthwhile — all asserted in host
  tests. This is the division the predictor wants: predictions as effects.
- **NMEA parsing lives in the core**, against sentences captured off the real receiver, because
  a GPS needs sky view and a slow cold start so deskbound iteration is the norm. Fixtures
  written from NMEA 0183 documentation were wrong — the unit speaks 4.1. Raw captures are
  gitignored: a fix records where and when someone was.
- **`crux_core` is pinned to `=0.16.2` on device.** On 0.19, Crux plus BLE rebooted the board
  every few minutes from inside crux's per-effect machinery. The pin fixes it; the cause was
  never identified, only avoided.
- **Hardware facts worth not re-deriving** — the power-hold pin, panel offset and bus ceiling,
  which Grove pin is RX, UART buffer sizing, and that stack overflow on this board always
  presents as a fault in unrelated code — are in `.claude/memory/m5-esp32-toolchain.md`.
- **GNSS noise is worse than the predictor's straw man assumes**: held still with poor
  satellite geometry, the receiver reported metres-per-second of phantom motion and a false
  multi-knot speed. Carried into the new "Deploy predictor on M5 device" slice, along with the
  crux pin as a constraint on the shared core.

