# Motis map/trips API (lookout "second source from Motis" slice)

Endpoint `GET /api/v1/map/trips` (canonical `/api/v6/`; server is version-tolerant).
Required params: `zoom`, `min="lat,lon"` (SW corner), `max="lat,lon"` (NE corner),
`startTime`/`endTime` (RFC3339); optional `precision`. Returns `TripSegment[]`
(stop-to-stop legs: `trips[]`, `mode`, `routeColor?`, `from`/`to` Place,
`departure`/`arrival`/`scheduled*`, `realTime`, Google-encoded `polyline`).

- **No vehicle-positions endpoint, in any German open feed.** `map/trips` gives
  *interpolated* position (walk the segment whose `[departure,arrival]` spans T along the
  decoded polyline), never a raw GPS dot. Both gtfs.de and DELFI RT are TripUpdates +
  ServiceAlerts only (0 VehiclePositions), so the best available is realtime-*corrected
  interpolation*, nationwide, not real GPS.
- **Realtime must be enabled** on the Motis server, via the dataset's `rt:`. **Key
  gotcha:** `motis server` reads the *expanded* `data/config.yml` that `motis import`
  writes — NOT the top-level `config.yml` (only `import`'s input). Editing top-level
  `config.yml` does nothing without a re-import. Motis has no config-override flag/env var
  (only client-side `MOTIS_BASE_URL`), so you edit the YAML: `just enable_rt` `yq -i`
  patches `.timetable.datasets.germanygtfs.rt` in `data/config.yml` (idempotent `=`; needs
  `brew install yq` via the `prerequisites` recipe), run after `import` in `motis_setup`.
  Restart `motis server` (no re-import; feed polled every `update_interval`=60s).
- **RT feed must match the static feed's ID namespace.** Pairing gtfs.de RT with a DELFI
  static timetable makes ~99.9% of trip updates fail to resolve (different trip/stop IDs).
  With DELFI static + DELFI RT (`https://germany.motis-project.org/gtfsrt`): 99.96% entity
  success, ~80% of Frankfurt-box segments `realTime:true`, delay-corrected.

## Feed = DELFI, not gtfs.de free (migrated 2026-07-23)

The static feed is now DELFI's nationwide "Sollfahrplandaten" (`opendata-oepnv.de`,
CC-BY-4.0; the "permanent link" is public — no token/secret; wget follows its 303 to a
dated `fileadmin` zip). Versus the old gtfs.de free feed this changes several facts the
pipeline depends on:

- **`route_type` is correct (101/102), so `mode` classifies.** Long-distance comes back as
  `HIGHSPEED_RAIL`/`LONG_DISTANCE`, regional as `REGIONAL_RAIL`, etc. (gtfs.de typed all
  rail as `route_type=2` → every train reported `REGIONAL_RAIL`). So `mode` alone now
  separates an ICE from an S-Bahn — agency is no longer needed for classification.
- **`poll_once` filters to rail modes** (`is_rail`: HighspeedRail/LongDistance/NightRail/
  RegionalFastRail/RegionalRail/Rail) before enrichment + storage, dropping
  bus/coach/tram/subway/metro.
- **Train numbers exist but are NOT on `map/trips`.** DELFI has `trip_short_name`
  (`002569`), but `map/trips`'s `TripInfo` exposes only `routeShortName` (the bare line,
  `"55"`) and `displayName` (`null` on raw DELFI — the formatted `IC 2569` is transitous'
  Lua fixup, not in the feed). Get the number from `GET /api/v4/trip` instead (below).
- **No IC/EC/ICE category anywhere in the feed.** DELFI `routes.txt` `route_desc`/
  `route_long_name` are empty for 101/102, and `route_type` can't split IC from EC. So we
  do NOT synthesise a category prefix (it would mislabel EC trains); `mode` carries the
  family honestly and `train_number` is the bare integer.
- **`shapes.txt` present but rail-useless → fixed with pfaedle.** DELFI ships a 308 MB
  `shapes.txt` (with `shape_id` in `trips.txt`), and Motis loads it (`with_shapes: true`) —
  but **only bus/coach operators populate geometry.** Rail `map/trips` polylines were ≤4
  points (straight stop-to-stop; COACH hits 1000+). Fixed by generating rail shapes with
  **pfaedle** — see the pfaedle section below.

## Rail track geometry via pfaedle (`tools/pfaedle`)

pfaedle map-matches GTFS trips onto OSM to synthesise `shapes.txt`. It's its **own tool**
under `tools/pfaedle` (a generic OSM+GTFS→shapes tool, not motis-specific), driven from
`tools/motis-server`'s `shapes` recipe.

- **No homebrew formula — built from source.** `just build` clones `ad-freiburg/pfaedle`
  and `cmake`-builds it to `build/pfaedle`. Needs `cmake` + `libzip` (`just prerequisites`);
  libzip lets it read/write the GTFS `.zip` directly. `-x` reads `.pbf` directly.
- **Run from the build dir, so pass `-c src/pfaedle.cfg`.** pfaedle only finds its default
  MOT→OSM matching config implicitly when installed; without it → "No MOT configurations".
- **`-D -m rail` recomputes rail shapes only, keeping bus/coach shapes.** Verified: rail
  `shp_101_*` shapes come out ~hundreds of points (curved); existing bus shapes untouched.
  `shapes.txt` grows 308 MB → ~2.3 GB; `trip_short_name` (train numbers) and `route_type`
  101/102 are preserved through pfaedle's rewrite.
- **pfaedle's GTFS parser is stricter than motis — drop `transfers.txt` + `pathways.txt`
  from its input.** It aborts on dangling references those leaf tables carry (DELFI's point at
  stop ids not in `stops.txt`), which motis tolerates. The splice (below) takes everything
  non-shape from the *original* feed anyway, so we just delete them from pfaedle's input
  (`HOLD_OUT`). Note: `trips.txt.shape_id` and `stops.txt.level_id` are validated too, so
  `shapes.txt`/`levels.txt` must stay *in* the input.

### 🚧 BLOCKED (2026-07-24): importing pfaedle's `shapes.txt` kills GTFS-RT realtime

**Do not ship the pfaedle pipeline as-is — it breaks realtime, and we don't yet know why.**
Slice parked here; the recipe/tool code is written but the produced feed is unusable.

- **Symptom:** import the raw DELFI feed → RT resolves ~99.97% (realtime ~90% of `map/trips`
  segments, delays present). Import any feed carrying pfaedle's `shapes.txt` → `trip_resolve_error`
  ≈ 99.6%, **0% `realTime:true`**. The static schedule itself imports fine (`map/trips` returns
  the trips, and rail is genuinely curved — HIGHSPEED ~543 pts, REGIONAL ~1050).
- **Isolated to `shapes.txt`, NOT `trips.txt`.** Three attempts, all broke RT identically:
  (1) pfaedle's full-feed rewrite; (2) splice via csv (reformats DELFI's quoting); (3) splice
  that keeps `trips.txt` **byte-identical to raw except the rail `shape_id` fields**
  (`splice/main.py`, right-split, verified only 163 431 lines differ). In (3) the failing RT
  trips are **bus** trips whose `shape_id` was never touched → the trigger is the swapped-in
  `shapes.txt` (308 MB → 2.3 GB), the only remaining difference from the working raw feed.
- **Not a live-feed currency issue.** Re-fetched the live RT feed same-day: its trip_ids still
  overlap our static 99.6% (218 446/219 368), so ids are present; motis just fails to resolve
  them once the big `shapes.txt` is imported.
- **Leading hypothesis (untested):** pfaedle's 2.3 GB rail geometry makes `motis import` hit a
  limit / produce a degraded timetable whose RT trip index is incomplete while scheduled
  queries still work. Alt: something in pfaedle's shape rows motis mishandles.
- **Why testing stalled:** the clean way to A/B is to import a feed, run `motis server`, read
  the `GTFS-RT update stats` line from its log. **Claude Code's sandbox can't run `motis server`**
  — it denies the LMDB tile mmap (`data/tiles/tiles.mdb` → "Operation not permitted"), and Mike
  doesn't want the sandbox disabled. So each RT test needs Mike to run motis by hand.

**To resume:**
1. Confirm the trigger: build `raw feed + only shapes.txt swapped` (trips.txt fully original),
   import, check the RT stat. Expect it to break → confirms `shapes.txt`.
2. Then chase the cause: inspect `motis import` stdout for shape/memory/limit warnings; try
   shrinking `shapes.txt` (pfaedle `--grid-size` / a Douglas-Peucker simplification of the rail
   polylines; and/or drop the unused 308 MB bus shapes), re-import, re-test.
3. If motis genuinely can't take large rail shapes, consider filing a motis issue, or accept
   straight-line rail (what transitous does — `drop-shapes: true`) and drop this slice.

**State to restore:** the running server's `data/` currently holds the RT-broken pfaedle import,
and `geo_data/germany_gtfs.pfaedle.zip` is the (RT-breaking) byte-preserving spliced feed. To get
working realtime back, re-import the raw feed: `just motis_setup germany_gtfs.zip` + restart.

- **Never overwrites the input.** `tools/motis-server` `just shapes` writes a derived
  `geo_data/germany_gtfs.pfaedle.zip` and re-imports via `motis_setup germany_gtfs.pfaedle.zip`
  (its `gtfs` arg defaults to the raw feed). The pfaedle-tool `shapes` recipe writes to a temp
  then atomically `mv`s to the output, so inputs are never touched and an interrupt can't leave
  a half-written zip. Heavy one-off (~16 min match over all-Germany rail + a big re-import).

## The `/trip` endpoint — agency + train number

`map/trips` segments carry neither agency nor train number. Both come from
`GET /api/v4/trip?tripId=…` (progenitor: `client.trip()` builder → `Itinerary` of `Leg`s;
`Leg.agency_name`/`agency_id`/`trip_short_name`/`route_type` are all `Option`). Pass
`join_interlined_legs=false` so a stay-seated trip isn't collapsed into one leg spanning
multiple agencies. `crates/motis/src/client.rs` `trip_details(trip_id) -> TripDetails
{ agency, train_number }` takes each field from the first leg that carries it;
`TrainNumber` wraps `NonZeroU32` (integer `trip_short_name`, leading zeros dropped;
`0`/`000000`/non-numeric → `None`). `poll::resolve_details` calls it per distinct
`tripId` each tick (stateless, no cache — Motis is local). Carried as an INTEGER
`train_number` column through the raw `segment` store and derived `train_segment`.

## Segment identity + client decisions

**Segment identity is `(trip_id, from_stop_id, departure)`, not `(trip_id, departure)`.**
Minute-resolution timetables let two legs of one trip depart *different* stops in the same
minute. So the ingest dedup key and the `train_segment` UNIQUE key both include
`from_stop_id`; keying on `(trip_id, departure)` silently drops legs. Rail
`train_segment.geom` is a 2-point WKB LineString (straight) until pfaedle.

Decisions: depend on the `motis-openapi-progenitor` crate (0.4.0, progenitor-generated,
reqwest 0.12, `.trips()` + `types::TripSegment`) rather than hand-write/generate in-repo;
decode polylines with the `polyline` crate. `.trips()` builder: `.zoom(f64)`,
`.min`/`.max(String)` (`"lat,lon"`, SW then NE), `.start_time`/`.end_time(DateTime<Utc>)`,
`.send().await` → `ResponseValue<Vec<TripSegment>>` (`.into_inner()`).

Two client gotchas (2026-07-19), handled in `crates/motis/src/client.rs`:
- **Connect to `127.0.0.1`, not `localhost`.** Motis binds IPv4 `0.0.0.0`; `localhost`
  resolves to IPv6 `::1` first, which never connects (looks like an empty result).
- **`map/trips` mis-parses fractional-second RFC3339 time bounds** — the response swings
  between empty and wildly oversized. Progenitor serialises `DateTime<Utc>` with micros,
  so the client truncates `start_time`/`end_time` to whole seconds before querying.

Schema changes to the raw `segment` / derived `train_segment` dbs are applied in place by
`crates/motis/src/migrate.rs` `ensure_column` (PRAGMA-checks then `ALTER TABLE ADD
COLUMN`), so the live poller's db gains a new column without a manual `ALTER`.

See [lookout-architecture](lookout-architecture.md).
