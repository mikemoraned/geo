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
- [x] **Ingest — dedup + decode into the `lookout` db.** A `motis_ingest` binary (in the
  `motis` crate) that reads the raw, duplication-allowed `motis.sqlite`, **dedups**
  segments on `(trip_id, from_stop_id, departure)` (the same scheduled leg re-seen across
  overlapping polls collapses to one row — prefer the newest `captured_at`'s realtime
  values), **decodes** each `polyline` (the `polyline` crate) to a lat/lon `LineString`
  stored as WKB, and writes a derived `train_segment` table into `data/lookout.sqlite`
  (idempotent `INSERT OR IGNORE`, mirroring `enrich`; WKB geom like the `transport`
  table). Keeps `trip_id`/`route_name`/`mode`/`route_color`/`realtime` and the
  realtime-corrected `departure`/`arrival`. Add a Justfile `ingest-motis` recipe
  (mirroring `enrich`). Unit test dedup + polyline decode over the captured fixture rows.
- [x] **Visualise the trains.** Extend `visualise/main.py`: read `train_segment`
  (time-windowed by `--since`), and for each timeline tick **interpolate** each train's
  position along its WKB line by `(t − departure)/(arrival − departure)` using the
  realtime-corrected times (shapely `line.interpolate(frac, normalized=True)` — shapely is
  already a dep) → a per-trip `GeoPoints` moving dot, coloured by `mode`/`routeColor`;
  optionally the static per-trip route line. Fold into the map blueprint so trains and GPS
  traces share the same view and window. Add Python tests in `visualise/tests`. Absent
  table → no-op (like `transport`).
- [x] **Verify the full pipeline.** `/verify` the whole chain end to end: poll → ingest →
  visualise, confirming the `.rrd` shows trains moving (using realtime-corrected timing)
  over the same window as the GPS traces (`rerun rrd verify` passes).


Note: positions are realtime-corrected *interpolation* (a second, non-GPS reference),
not raw vehicle GPS — the best the gtfs.de TripUpdates feed allows nationwide. Consider
filtering to rail-ish modes (drop `BUS`) in the poll or ingest step so the visualisation
is trains, not all transit.

## Pending refactors / improvements

* [x] switch all geo types (e.g. BBox) to instead use shared types from external Rust geo libraries where possible; let's avoid inventing something we can re-use
* [x] use the `wkt` crate to build the Overture query-window WKT (`transport::overture::bbox_ring_wkt` / `bboxes_multipolygon_wkt`) instead of hand-formatting the `MULTIPOLYGON` string
* [x] read the `train_segment` geom via the `wkb` reader in the `motis::ingest` test, instead of hand-parsing the WKB header bytes
* [x] use `geo` algorithms (`BoundingRect` / `Scale`) for `PositionWindow::bbox` and `buffered_bbox` instead of hand-rolled min/max + centre arithmetic
- [x] **Capture `agency_name` so long-distance trains are separable.** The feed types all
  rail as `route_type=2`, so Motis reports ICEs as `REGIONAL_RAIL` and `mode` cannot tell
  an ICE from an S-Bahn (see Investigation). `map/trips` doesn't carry agency — verified
  against the generated `TripSegment`/`TripInfo` types (12 and 3 fields, no agency, no
  `additionalProperties`) and against 101 live segments. It's only on
  `GET /api/v1/trip?tripId=…`, whose legs expose `agencyName`/`agencyId` alongside
  `routeType` and `routeShortName`.

  The crate already covers this: `client.trip()` → `builder::Trip` with `.trip_id(…)` and
  `.send() -> Result<ResponseValue<types::Itinerary>, Error<()>>`, and
  `Itinerary.legs[].agency_name`/`agency_id` are `Option<String>`. Note it defaults to
  `join_interlined_legs=true`, collapsing a stay-seated trip to one leg whose agency spans
  the whole trip; pass `join_interlined_legs(false)` if per-segment exactness matters.

  So: add a `trip_details(trip_id)` call to `MotisClient`, and in `motis_poll` resolve
  each distinct `tripId` in the tick before appending, fresh each tick — no caching. Motis is
  local and the poll interval is coarse, so the calls are cheap and the loop stays
  stateless. A resolve failure must not drop the segment — log and store `NULL` agency.

  Store `agency_id`/`agency_name` on the raw `motis` rows, carry both through
  `motis_ingest` into `train_segment`, and let the visualisation filter/colour on
  `DB Fernverkehr AG` rather than mode alone.
  > **Notes:** the client method is named `trip_agency(trip_id) -> Option<Agency>` (it
  > returns only the agency, not full trip details). The visualisation *colours* on
  > agency — a `DB Fernverkehr AG` train gets the long-distance red even though `mode`
  > says `REGIONAL_RAIL` (routeColor still wins when present). `store`'s schema-on-open
  > `IF NOT EXISTS` won't add columns to a pre-existing raw `motis.sqlite`, so the tracked
  > db was `ALTER TABLE`d to add the two columns (old rows keep `NULL` agency).
### Feed sources (from the transitous comparison — see Investigation)

Context for the group below: `api.transitous.org` returns strictly richer data than our
instance (train numbers, `route_type` 101/102) **because it imports a different dataset —
DELFI — not because it is configured better.** No setup step is missing on our side.

Two negative results worth not re-deriving:
- gtfs.de's dedicated long-distance feed (`download.gtfs.de/germany/fv_free/latest.zip`)
  is **no better** than the combined free feed: same 3-column `trips.txt`, all 96 routes
  `route_type=2`. No rearrangement within the gtfs.de *free* tier yields train numbers.
- **No German open feed carries VehiclePositions.** The DELFI RT feed decodes to 235,301
  entities, 100% `trip_update`, zero `vehicle`. Realtime-corrected interpolation is the
  nationwide ceiling — that constraint is structural, not a gtfs.de artifact.

Considered and *not* proposed as work, recorded so it isn't re-researched: **gtfs.de paid
tiers** ("Complete Feed"/"Complete Feed Plus" — the site publishes no detail on train
numbers or shapes, and DELFI is free and demonstrably sufficient); **DELFI NeTEx** (same
data, richer model, more import work for no gain here — GTFS is what Motis wants);
**DELFI ZHV** (stop registry only, no registration, irrelevant to trip data); **regional
feeds** (VBB/NRW/MobiData BW — better locally, no long-distance help).

- [ ] **Switch the RT feed to DELFI's.** Cheap and independent of everything else: the
  `enable_rt` recipe's `https://realtime.gtfs.de/realtime-free.pb` becomes
  `https://germany.motis-project.org/gtfsrt` (CC-BY-SA-4.0, no registration — one of three
  mirrors transitous lists). Broader coverage than the gtfs.de free RT feed. Still
  TripUpdates-only. Verify the `realTime: true` / delay-corrected ratio against the current
  201/221 baseline.
- [ ] **Evaluate importing the DELFI static GTFS.** The headline upgrade: real train
  numbers (`trip_short_name`) and correct `route_type` 101/102, so ICEs report as
  `HIGHSPEED_RAIL`/`LONG_DISTANCE`. Source: "Deutschlandweite Sollfahrplandaten (GTFS)" at
  opendata-oepnv.de, CC-BY-4.0, refreshed Mondays, **requires free registration** (the
  transitous config URL is a share link, not a plain download) — so `download_geo_data`
  can't fetch it unattended; plan for a manual/credentialed step. Nationwide incl.
  long-distance. Verify: our instance shows `IC 2569`, not `IC 55`, for the 21 Jul train.
- [ ] **Check whether DELFI ships `shapes.txt` — may retire the pfaedle task.** Transitous
  sets `"drop-shapes": true` on their DELFI source, which strongly implies the feed *has*
  shapes (you don't drop what isn't there), but this is inference — unverified behind the
  registration wall. Settle it on first download. If DELFI has usable shapes, the pfaedle
  task below is unnecessary; if transitous drops them for a reason (size/quality), find out
  which before relying on them.
- [ ] **Format DELFI train numbers on import.** Raw DELFI `trip_short_name` is unformatted
  (e.g. `00123`); transitous' `scripts/de-DELFI.lua` strips leading zeros and prefixes the
  line type (`ICE 123`/`IC 123`) off `route_type`, and fills missing route colours. Decide
  whether to reproduce that. **Unverified: whether Motis supports per-dataset Lua fixups
  from a plain `config.yml`** — transitous may apply it in their own build pipeline. Check
  before assuming; the fallback is normalising in `motis_ingest` instead.
- [ ] **Revisit agency-based classification once DELFI is in.** With correct `route_type`
  101/102, `mode` alone distinguishes an ICE from an S-Bahn, so the per-trip `trip_agency`
  lookup added above may become redundant for *classification* (it stays useful as
  operator metadata). Re-evaluate rather than delete — decide whether the extra call per
  distinct trip still earns its place.
- [ ] **(Quality improvement) Generate real track geometry with pfaedle.** The gtfs.de
  feed has no `shapes.txt`, so `map/trips` polylines are straight stop-to-stop lines and
  interpolation cuts corners. In `tools/motis-server`, add a Justfile step that runs
  `pfaedle` (rail-only) over the OSM + GTFS to produce shapes and repackage the feed, e.g.
  `pfaedle -x geo_data/germany.osm.pbf -m rail -X geo_data/filtered.osm.pbf geo_data/germany_gtfs.zip`,
  then zip `gtfs-out/` back over `germany_gtfs.zip` and re-run `motis import` + restart.
  Heavy one-off run. Verify: `map/trips` polylines now decode to many points (curved
  track), not 2, so the trains follow the rails. Independent of the pipeline above — a
  drop-in geometry upgrade the visualisation picks up automatically on the next ingest.
  > **Do the DELFI `shapes.txt` check first** — if DELFI ships shapes this task is moot.

## Observations

### On train on Sunday 19th July from Ronneburg to Mannheim Hbf

I am on train ICE 693 from Aschaffenburg Hbf to Mannheim Hbf (which is the ICE 693 to Munchen Hbf). However, this doesn't show up on motis in UI e.g. arrivals at that time (20:30) shows ICE 11. Note that I also didn't find this train earlier when doing a search for routes from Ronneburg to Mannheim. Is it possible the timetables are incomplete/wrong?

Also: when observing "speed" on gps on my phone, it commonly shows as "40.<something>". This seems slow for a train even interpreting as miles per hour.

### On train on Tuesday 21st July from Mannheim Hbf to Koblenz Hbf

I am on train 08:39 from Mannheim Hbf to Koblenz Hbf, IC2569, leaving at 08:39 and arriving at 10:11. A train on this route and with these departure/arrival times is showing up in Motis when I do a [search](http://localhost:8080/?time=2026-07-21T06%3A30%3A00.000Z&fromPlace=germanygtfs_503494&toPlace=germanygtfs_309638&withFares=true&numLegAlternatives=3&fastestDirectFactor=1.5&joinInterlinedLegs=false&maxMatchingDistance=250&fromName=Mannheim%2C+Hauptbahnhof%2C+Mannheim%2C+Baden-Württemberg%2C+Germany&toName=Koblenz+Hauptbahnhof%2C+Koblenz%2C+Rhineland-Palatinate%2C+Germany) but with train id IC55.

#### Investigation

Both observations have the same cause: the gtfs.de free feed identifies long-distance
trains by **line**, not by train number. Neither train was missing.

- `routes.txt` carries a small fixed set of long-distance routes named for the DB
  Fernverkehr Netzplan lines — 47 `ICE nn` and a handful of `IC nn`/`EC nn`. ICE 693
  (Berlin–Frankfurt–Mannheim–Stuttgart–München) runs on line **ICE 11**; IC 2569 runs on
  line **IC 55**. Those are the labels observed, and both trips are present in the
  timetable at the observed times (Mannheim arr 20:30 dep 20:33 → München Hbf on the
  19th; Mannheim dep 08:39 → Bielefeld Hbf on the 21st).
- The train number cannot ever be surfaced: `trips.txt` has only
  `route_id,service_id,trip_id` — no `trip_short_name`, no `trip_headsign`. Confirmed via
  the API that `tripShortName` is empty on every long-distance stop time. Motis derives
  the headsign it shows from the trip's last stop.
- **Consequently the timetables are not incomplete or wrong, just anonymised to line
  level.** Recovering real train numbers needs a richer feed than gtfs.de free — the same
  tradeoff as the missing `shapes.txt`.

Incidental finding that shapes the pipeline: every rail route is `route_type=2` (generic
Rail); there is no 101/102 for high-speed. Motis therefore reports ICEs as
`REGIONAL_RAIL`, and a `mode=HIGHSPEED_RAIL,LONG_DISTANCE` query at Mannheim Hbf returns
zero results. So `mode` can drop `BUS` but cannot separate an ICE from an S-Bahn; the
usable discriminator is `agencyName` (`DB Fernverkehr AG`), which the store does not
currently capture.

The route-search half of the 19th's observation is unexplained and likely unrelated — the
timetable demonstrably holds the train, so it points at routing behaviour (Sunday
service, transfer constraints, or which "Ronneburg" was geocoded) rather than data. Not
pursued.
