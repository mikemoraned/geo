# Current Slice: Motis as ground truth

### Target

Before I continue down the route of relying on Transitous, I'd first like to try bringing my local
Motis setup up to scratch, as at least a source of correct geo routes and correct train arrival
times. The focus is establishing a ground truth with large coverage.

We should get new motis data by polling motis live in a particular bbox and watching when trains arrive. The idea is that we need to get enough data that we can put together a reasonable size test dataset, and *also* that we gather enough data to do more ambitious stuff with it later, where we use the motis data as effectively input data for a model or a dataset.

The slice "Evaluation framework with associated Crow Flies predictor improvement" scores predictions
against this ground truth.

### Decisions

Decisions taken before starting, as each changes what gets built.

#### Upgrades

- **geo, then crux, before the slice's own work, each in its own commit.** The slice changes
  both.
- **geo 0.32, ahead of SedonaDB.** The host tree also carries geo 0.31, for sedona-geo, until
  the SedonaDB 0.5.0 slice. The device tree carries 0.32 alone. Narrowed on 2026-10-03 from
  0.33.1: geo 0.31 caps `i_overlay` below 4.1, geo 0.33 needs 4.5.1 or later, and one build holds
  one 4.x.
- **crux_core 0.20.0.** The reboots of 2026-09-21 struck on 0.16.2 too. The 0.20 fault in
  `docs/device.md` was on the 1.90.0 toolchain, and 0.20 has not run on esp-1.98.1.0. Kept on
  2026-10-05 despite a double exception in the soak, since 0.16.2 crashes as well.
- **A 30-minute soak after the crux move, against the soak of 2026-10-03.** Both record reboots,
  unused stack, free heap, and the slowest sentence, under the same conditions. The two differ by
  geo as well as crux. Narrowed on 2026-10-04 from a fresh baseline after the geo move: the geo
  move is not expected to affect stability, and a build and the Rust tests cover it.

#### Ground truth

- **A pass per visit, at the visit's nearest sample.** `match_crossings` kept one pass per
  (session, crossing), so a session reversing over a crossing passed it once. It instead splits
  the samples within the match radius into visits, with a pass at each visit's nearest sample.
  Narrowed on 2026-10-19 from an instant interpolated between the bracketing samples.

#### Motis capture

- **A bronze source table** records the server, its feed, its Motis version, and its area. Each
  captured row carries its source's id.
- **Each capture version derives its own silver version.** New captures go to `motis_segment_v2`,
  and `train_segment_v2` derives from it, every row naming its source. `motis_segment` stays as
  written, and `train_segment` derives from it with no source. A reader wanting every train reads
  both. Changed on 2026-10-07 from one silver dataset with an optional source id. Superseded on
  2026-10-18 by the group One train_segment version: both capture datasets derive into
  `train_segment_v2`, the earlier captures under two legacy sources.
- **The local server is the sampling source, and Transitous checks it.** With rail legs that
  follow the track, the local server covers Germany without loading a volunteer-run service.
  Transitous's shapes and times for the same DELFI trips check that it reaches the Target.
- **Polls to Transitous are polite.** One poll every 10 minutes, over Thuringia alone. Each request
  carries a `User-Agent` naming lookout, its version, and `https://github.com/mikemoraned/geo`, as
  the usage policy requires. A train's details are fetched once per run, not once per poll.
  Confirmed 2026-10-06.
- **Anything showing Transitous data links to https://transitous.org/sources/**, as the usage
  policy requires.
- **S-Bahn stays out of the capture.** The poller keeps mainline and regional rail, and drops
  `SUBURBAN`, almost half of a country-wide response. Confirmed 2026-10-06.
- **`motis_poll` gains an area mode**: a country and an optional region, such as DE and
  Thuringia. It queries the area's bounding box, from Overture's `division_area`, and keeps the
  legs touching the area and lying wholly within its country. Changed on 2026-10-18 from every leg
  touching the area: the local server routes over one country's OSM extract, and draws a leg
  straight beyond its border.
- **A source's id is a UUID v5 of its fields**, as a session's is. Registering a source twice
  writes one row, and a Motis upgrade makes a new source. Confirmed 2026-10-05.
- **The poller asks the server for its Motis version**, from `serverConfig.motisVersion` in
  `map/initial`. No endpoint names the feed, so `--feed` does. Confirmed 2026-10-05.
- **A region is recorded as its Overture division id and its name.** The id joins to the extract,
  and the name is for a reader. Confirmed 2026-10-05.
- **The Motis client is generated here, from the `openapi.yaml` of the pinned Motis version.**
  `motis-openapi-progenitor` 0.4.0, the latest, targets API v4 and has no `serverConfig`.
  `progenitor` reads OpenAPI 3.0 alone, so the build declares the 3.1 spec as 3.0.3, which
  generates for v2.11.3. Confirmed 2026-10-05.
- **A source carries an optional free-text label**, set by `--label` beside `--feed`, and the id
  hashes it when present. The label holds whatever tells two setups of one server apart, such as
  the DELFI build and whether shapes are computed. The alternatives fix the meaning in new fields,
  or hash the server's `config.yml`, which no reader can interpret. A source without a label keeps
  its id. Confirmed 2026-10-12.

#### A complete local server

- **The import computes rail shapes with `route_shapes`, in mode `all`.** Every DELFI trip
  carries a shape, so mode `missing` skips rail. Mode `all`, with every class but high-speed,
  long-distance, night, and regional turned off, recomputes the classes the capture keeps and
  leaves DELFI's other shapes alone. Transitous instead drops the feed's shapes and runs
  `missing`. The block goes into `config.yml` between `motis config` and `motis import`.
- **The import loads Transitous's `de-DELFI.lua`**, as the DELFI dataset's `script`, so a train's
  name reads as Transitous's does, such as `ICE 123`. A recipe fetches it from Transitous's
  repository at a pinned commit, rather than vendoring an AGPL-3.0 file into the tree. The
  captured line name changes with it, since bronze takes `displayName` before
  `routeShortName`. Confirmed 2026-10-12.
- **SIRI ET waits on the measurement.** Transitous adds DELFI's SIRI ET forecasts to the
  trip-update feed both servers poll. The local server takes them only if the side-by-side shows
  times disagreeing on trips Transitous corrects. Confirmed 2026-10-12.

#### Side-by-side check

- **Two side-by-side passes, one before the fixes and one after.** Each polls the local server
  and Transitous together, over Thuringia, every 10 minutes, for 2 hours. The first measures how
  far an unchanged local server is from Transitous, so the second shows what the fixes moved.
  Confirmed 2026-10-12.
- **Both servers import the same weekly DELFI build.** A leg from each joins on DELFI's own trip
  id, the suffix of both servers' trip ids, with the departure stop and time. Transitous imports
  the latest build, so the local server takes it fresh before each pass.
- **The comparison is per joined leg**: the distance between the two shapes, and the difference
  between the two realtime arrivals. Legs only one server returns are counted too. What counts as
  agreement comes from the baseline's distributions, not from this decomposition.
- **Two shapes agree when they lie within 15 m of each other**, rounded up from the largest shape
  distance in the route-shapes pass, 12.6 m. The baseline could not set it, since every local shape
  there runs straight between its stops. Confirmed 2026-10-17.

#### Compact capture

The measurements behind the Compact capture decisions are in
`2026-10-23-compact-bronze.md`, since deleted.

- **Polls land in landing, and a drain moves them into bronze.** Bronze never rewrites a file, and
  git keeps every committed blob, so polls merge before they reach git. Merging poll files after
  committing them frees nothing. Landing sits at `data/medallion/landing/`, ignored by git.
  `docs/medallion.md` already describes landing as the capture log a polling process appends to,
  drained into bronze. Confirmed 2026-10-23.
- **A drain writes the polylines bronze lacks, then the polls.** It appends the polylines
  `motis_polyline_v1` lacks as one new file, each keyed by a UUID v5 of its text, as a source's id
  is. It then writes the polls to `motis_segment_v3`, one zstd file per `polled_date`, each row
  carrying a polyline id in place of its polyline. Rows sort by stop pair, polyline id, trip, and
  capture time. Draining every 10–24 hours, a day then takes 5.4–7 MB in git, against 58–70 MB as
  poll files. Appending every polyline a drain sees, rather than the missing ones, adds at least 7
  MB a day. Confirmed 2026-10-23.
- **A drain is safe to re-run.** It writes polylines before polls, reads both back, and only then
  deletes the landing files. A re-run writes whatever a crash left in landing again, so bronze can
  hold a poll twice, and the derivation to silver collapses the repeat. A crash then leaves at worst
  an unused polyline or a doubled poll, never a lost one. Confirmed 2026-10-23, revised 2026-10-31.
- **`writer_at` compresses with zstd, for bronze and silver alike.** It writes both layers, so
  compressing both needs no writer properties per layer, as compressing bronze alone would. It also
  shrinks silver on disk, which an ingest run needs room for. Confirmed 2026-10-23.
- **The running capture finishes as it started.** The poller keeps writing `motis_segment_v2` until
  about 25 October, committed and pushed as now, as a backup the rewrite later drops. Stopping it
  mid-capture risks a gap. Confirmed 2026-10-23.
- **A one-off binary migrates `motis_segment_v2` into `motis_segment_v3` and `motis_polyline_v1`.**
  It is committed with the data it writes, then removed, as earlier migrations were. The drain then
  handles landing alone, rather than reading `motis_segment_v2` as it reads landing. Once nothing
  reads `motis_segment_v2`, its files and code support go. Confirmed 2026-10-23.
- **This branch drops its poll files from history before it merges.** A merge keeps a branch's
  commits, so `main` would hold the poll files for good. They take 316 MB packed now, and about
  500 MB once the capture ends. `main` holds none of them today, and no other branch carries them,
  so the rewrite touches this branch alone, with a force-push. A squash merge would also leave them
  out, but loses the slice's commit history. Confirmed 2026-10-23.

#### Consequences elsewhere

- `docs/device.md` and the pin comment in `crates/platform/web/core/Cargo.toml` say crux after
  0.16.2 reboots the device.
- The workspace `Cargo.toml` comment ties geo to sedona-geo's version. The SedonaDB 0.5.0 slice
  moves geo from 0.32 rather than 0.31.
- `App` in `crates/platform-core/src/app.rs` loses `type Capabilities` and the `_caps` argument.
- `silver/session_crossing` can hold several rows per (session, crossing), so the packed sessions
  and `sessions.version` change.
- The derivation graph in `docs/architecture.md` changes.
- `docs/motis.md` says `displayName` is null on raw DELFI. With the script loaded, the local
  server fills it.

#### Open questions

None remain.

#### Rejected / deferred

- **pfaedle.** Its rail shapes followed the track, but importing a feed carrying them broke
  realtime, for a reason never found. Motis's `route_shapes` import step computes shapes and leaves
  the feed alone. Its line-relation penalty only matters for placing a train on the right one of
  two parallel lines, which lookout does not need. Rejected on 2026-10-17.
  [2026-10-12-track-shapes-and-realtime.md](2026-10-12-track-shapes-and-realtime.md) keeps what the
  attempt established.
- **Snapping Motis legs to rail in lookout's own code.** It repeats what `route_shapes` does, and
  meets the same wrong-parallel-line problem.

### Tasks

#### Upgrades

- [x] Move `geo` to 0.33.1 in the workspace, `crates/domain`, and `crates/predictor`, and rewrite
      its pin comment.
      Moved to 0.32 instead. 0.33 does not resolve alongside SedonaDB 0.4.1.
- [x] Run `just test-no-docker`, `just test-geo`, and `just m5plus-build-release`.
- [-] Soak the release build for 30 minutes, as the baseline. The user runs this.
      Moot: the soak of 2026-10-03 serves as the baseline.
- [x] Move `crux_core` to `=0.20.0` in every crate pinning it, and drop `Capabilities` and `_caps`
      from `App`.
      `BridgeWithSerializer` is gone as well. The web bridge and a core test use `Bridge` with
      `JsonFfiFormat`, and the JSON a shell sees is unchanged.
- [x] Run `just test-no-docker`, `just wasm`, and `just m5plus-build-release`.
      `just wasm` exposed a break from the geo move: geo 0.32 depends on `rand`, whose `getrandom`
      builds for the browser only with its `js` feature. `web-bridge` now enables it.
- [x] Soak for 30 minutes under the conditions of the soak of 2026-10-03. The user runs this.
      Four double exceptions in 48 minutes, each matching a known 0.20 signature at the same
      instruction. The sentence that scans took about 262 ms throughout, against 8.3–8.7 ms on
      0.16.2. On the host, neither crux 0.20 nor geo 0.32 changes that sentence's time.
      A further soak on crux 0.16.2 with geo 0.32 gave one double exception, at the same
      instruction, in 52 minutes, and 119 ms for that sentence.
- [x] Rewrite the crux section of `docs/device.md` and the `web/core` pin comment from the soaks.
- [x] Note in the SedonaDB 0.5.0 slice in `docs/next-slices.md` that geo moves from 0.32.

#### Motis capture

Next, so data accumulates while the rest is built.

- [x] Measure one `map/trips` poll over DE at zoom 8: its size, its legs, and its duration.
      7.1 MB, 7,146 legs, and 0.23 s, at 09:00 on a weekday. One request covers DE.
- [x] Vendor v2.11.3's `openapi.yaml` in `crates/motis`, and generate the client from it in
      `build.rs` with `progenitor`.
      Vendored as `openapi.json`, converted with `yq`, so the build reads it without a YAML crate.
- [x] Move `crates/motis` to the generated client and the v6 endpoints, and drop
      `motis-openapi-progenitor`.
- [x] Add a recipe fetching `openapi.yaml` for the version `tools/motis-server/Justfile` pins.
- [x] Add `MotisSourceRow` and the bronze dataset `motis_segment_v2` to `crates/medallion-model`.
- [x] Register the source in `motis_poll` at startup, writing it only when absent.
      The capture log writes `motis_segment_v2` from here on, so ingest moved with it.
- [x] Send the `User-Agent` on every request the client makes.
      `lookout/<crate version>+<git commit> (+https://github.com/mikemoraned/geo)`. The commit
      comes from `BUILD_GIT_HASH`, as the server crate's does.
- [x] Cache each trip's details in `motis_poll` for the run, so a train costs one `/trip` request.
      A failed lookup is reported in the poll's outcome and asked again at the next poll.
- [x] Refactor `crates/motis` where a function takes too many arguments or does too much.
      `poll_once` takes seven, and groups none of them into a type that means something.
      Look for the same in the rest of the crate.
      `NearGps` decides where to look and `Capture` records what Motis shows there. `poll_once`
      and `PollConfig` are gone. A position holds an instant and a point, not raw numbers.
- [x] Read both datasets in `motis_ingest`, and carry `source_id` into `TrainSegmentRow`.
      Each derives its own silver version: `TrainSegmentRowV1` from `motis_segment`, and
      `TrainSegmentRowV2`, with a required `source_id`, from `motis_segment_v2`.
- [x] Add the area mode to `motis_poll`, and a recipe for it beside `bronze-poll-motis`.
      `--overture-country` and `--overture-region` take Overture's own codes, such as DE and
      DE-TH. `just bronze-poll-transitous` polls Thuringia every 10 minutes. Against the local
      server one poll of Thuringia kept 56 of 140 rail legs in its bounding box.
- [x] Split `motis_poll` into two binaries sharing `Capture`: `motis_follow_gps`, which follows
      an area that moves with recent GPS, and `motis_track_area`, which tracks one fixed Overture
      country or region over time. Point `bronze-poll-motis` and `bronze-poll-transitous` at them.
- [x] Refactor `crates/motis` again once the area mode lands: a function taking too many
      arguments or doing too much, a hand-rolled type a library or `std` provides, a raw number
      standing in for a type.
- [x] Log, for each poll, how many requests of each kind it sent to Motis and how long each kind
      took in total. A capture of Thuringia on 2026-10-11 took 5.0, 4.7, and 8.9 minutes per poll,
      though the second and third looked up only 8 and 16 new trips, so the trip lookups alone do
      not explain the time.
- [x] Run `just bronze-poll-transitous 30m`, three polls, and read each poll's request counts and
      times from its log. The user runs this. If the cache works, each later poll sends far fewer
      `trip` requests than the first. If the request times fall well short of the poll's time, the
      rest goes on writing the capture log and filtering legs.
      On 2026-10-12 the cache worked: the three polls sent 40, 6, and 2 `trip` requests. Each
      poll took 382–392 s, of which its requests took at most 3.3 s. The time held steady as the
      legs kept fell from 83 to 67, while each poll tested 164–171 legs against the area.
- [x] Log, for each poll, how long it spent filtering legs against the area and writing the
      capture log, beside the request times.
      On 2026-10-12 a dev build's poll took 483 s: 480.8 s filtering 147 legs, 2.2 s on
      requests, and 18 ms writing the log.
- [x] Build the Motis pollers with `--release` in their recipes, and measure the filter again.
      On 2026-10-12 a release build's poll took 55.8 s: 53.7 s filtering 89 legs, 0.6 s a leg
      against the dev build's 3.3 s.
- [x] If the filter still takes more than a few seconds a poll, make the test of a leg against the
      area cheaper: reject a leg clear of the area's bounding box first, and test the rest against
      a simplified or prepared outline.
      Prepared alone: geo's `relate` already rejects disjoint bounding boxes, and needed no
      simplified outline. On 2026-10-12 a poll took 2.1 s, 0.25 s of it filtering 73 legs, 3.5 ms
      a leg against 0.6 s before.
- [x] Record in a source's area the Overture release its shape came from. A new release can move
      a region's outline, yet a capture at it now registers under the same source id.
      The id covers the release from here on. Only this branch has written `motis_source`, so the
      change is in place, with no new dataset version:
      1. Add the release to `Area` and `MotisSourceRow`, and to the fields the id hashes.
      2. Backfill the one existing source as two, with a migration binary committed and then
         removed: release `2026-07-22.0` for the three captures of 2026-10-11, and `2026-09-23.1`
         for the seven of 2026-10-12. Rewrite `source_id` in `motis_source` and in the ten
         `motis_segment_v2` files to the new ids, and derive `train_segment_v2` again.
      3. Read the backfilled sources and segments back through Rust.
      4. Run a short capture of Thuringia, and check it finds the `2026-09-23.1` source already
         recorded. The user runs this.
      Two sources, not one: the `2026-09-23.1` extracts reached this branch at 23:00 UTC on
      2026-10-11, after the first captures and before the rest. DuckDB was dropped because it
      cannot write the store's Parquet types, such as required columns and UTC milliseconds.
- [x] Add an optional `label` to `MotisSource` and `MotisSourceRow`, hashed into the id only when
      present, and a `--label` flag to `motis_track_area` and `motis_follow_gps`.
- [x] Describe the source and `motis_segment_v2`, the area mode, Transitous, and its usage policy
      in `docs/motis.md`.

#### Baseline side-by-side

The local server as it stands, against Transitous. Only after `--label` lands.

- [x] Add a recipe `bronze-poll-local-area` beside `bronze-poll-transitous`: the local server,
      the same area and interval, and a `--label` naming the DELFI build and the setup.
      The label is the recipe's first argument, since the build changes with each pass.
- [x] Run `download_geo_data` and `motis_setup` in `tools/motis-server` for the latest DELFI build,
      with no other change. The user runs this, outside the sandbox.
- [x] Run `bronze-poll-local-area` and `bronze-poll-transitous` together for 2 hours. The user
      runs this.
- [x] Count the joined legs, and the legs only one server returned, in a new notebook
      `notebooks/motis_capture/side_by_side.py`. Built with the user in a live marimo session.
      Renamed `side_by_side_delfi-2026-10-10.plain.py`, after the label of the pass it compares.
- [x] Plot, per joined leg, the distance between the shapes and the difference between the
      realtime arrivals, by mode.
- [x] Set the agreement thresholds from those distributions.
      Two arrivals agree when they differ by at most 1 minute, from the 145 regional legs both
      servers had realtime for. The shape threshold waits for the fixed pass: every local shape in
      the baseline runs straight between its stops.
- [x] Record the local server's realtime resolution from its log, against the 99.96% in
      `docs/motis.md`.
      99.966% in each of eight updates from 12:00 to 12:08 UTC on 14 October: 229,392 to
      230,803 entities an update, 77 or 78 of them unresolved. The log covers the day after the pass, on
      the same server and build. The figure matches `docs/motis.md`.

#### A complete local server

Only after the baseline pass, so the fixes land on a measured starting point.

- [x] Add the `route_shapes` block to `config.yml` with `yq` in `motis_setup`, between
      `motis config` and `motis import`.
      The block turns off funicular as well as the classes in the research doc's block, since
      funicular takes the railway profile too. Motis accepts an unknown class name without error,
      so a misspelt class goes on being computed without warning.
- [x] Add a recipe fetching `de-DELFI.lua` from Transitous's repository at a pinned commit, and
      set it as the `germanygtfs` dataset's `script` in `motis_setup`.
      The script loads `scripts/de-delfi-colors.lua` with `require`, which Lua resolves against
      the directory the import runs from. The recipe fetches both into `motis_server/scripts`.
- [x] Run `motis_setup` on the same DELFI build. The user runs this.
      On 2026-10-17, with the DELFI build of 2026-10-10. The import log shows the script loaded,
      with handlers for agencies, routes, and trips.
- [x] Record the import's time, peak memory, and computed shape files' size, and the routes
      matched, segments routed, and segments beelined from the import log.
      The import took 6 minutes 8 seconds, with a peak resident set of 5.1 GB. The street graph,
      geocoding, and tiles were reused from the import of 2026-10-13, so the timetable and
      `route_shapes` took most of it. `route_shapes` took 3 minutes 17 seconds. The timetable
      took 1 minute 44 seconds, against 39–43 seconds without the script. The computed shapes take
      1.46 GB, and their cache 26 MB. 24,029 routes matched, 261,025 segments routed, and 16,385
      segments beelined: 5.9% of the segments run straight between their stops.
- [x] Check realtime resolution against the baseline's.
      99.977–99.979% in each of eight updates from 10:37 to 10:44 UTC on 17 October, against the
      baseline's 99.966%: 143,996 to 144,152 entities an update, 31 to 33 of them unresolved.
      Computed shapes leave realtime resolving, unlike pfaedle's.
- [x] Check `displayName` on a local `map/trips` leg reads as Transitous's does.
      On 2026-10-17 at 10:40 UTC, every rail leg over Thuringia carried one: 28 high-speed legs, such
      as `ICE 1007`, and 108 regional legs, such as `RB1 (20814)`. Long-distance legs read as
      `IC 2065` and `FLX10`. The shapes follow the track, with a median of 140 points a regional
      leg and 1,812 a high-speed leg, against 114 and 2,170 on Transitous on 2026-10-11.

#### Fixed side-by-side

Only after the complete local server is running.

- [x] Download the latest DELFI build, and run `motis_setup` again if Transitous has moved to it.
      The user runs this.
      Unchanged on 2026-10-17: Transitous last updated DELFI at 07:36 UTC on 12 October, so both
      servers stay on the build of 2026-10-10.
- [x] Run `bronze-poll-local-area`, labelled for the new setup, and `bronze-poll-transitous`
      together for 2 hours. The user runs this.
- [x] Copy `notebooks/motis_capture/side_by_side_delfi-2026-10-10.plain.py` to
      `side_by_side_delfi-2026-10-10.route-shapes.py`, point it at the new captures, and compare
      each distribution with the baseline's. The baseline notebook keeps its own captures, and
      changes only where the method does.
      The comparison is a notebook of its own, `compare_delfi-2026-10-10.plain_route-shapes.py`.
      Every joined leg of the route-shapes pass lies within 12.6 m of Transitous's shape, and
      regional arrivals agree. The local server is usable as ground truth, confirmed 2026-10-17.
- [-] Decide on SIRI ET from the arrival differences on trips Transitous corrects. If it is
      needed, add the feed to `enable_rt` and run a third pass.
      Deferred on 2026-10-17: the local server goes without SIRI ET for now. In the route-shapes
      pass, only ICE 599 differed from Transitous by more than 13 minutes, by up to 38. The slice's summary in `completed-slices.md` names
      this gap.
- [-] Compare the computed shapes with recorded sessions in `silver/session` on German lines, and
      count the legs drawn along a parallel line.
      Moot on 2026-10-17: the recorded sessions are too few to base a decision on. Lookout also
      has no need to place a train on one of two parallel lines.

#### Country-wide capture

Only after the fixed pass agrees.

- [-] Measure one `motis_track_area` poll of DE against the local server, and set the interval
      from its duration.
      Moot on 2026-10-18: DE polls every 10 minutes, as Thuringia does. Each poll's window now
      spans its interval, so a shorter interval buys only fresher forecasts. The first poll of the
      day's capture logs its duration.
- [x] Add a recipe `bronze-poll-local-de` for DE at that interval.
- [x] Capture DE for a day. The user runs this.
      - The count of trains per poll peaks at 1,630, at 09:42 UTC on Sunday 18 October: 1,384
        regional, 191 high-speed, and 55 long-distance. A poll counts every trip running at any
        point in its 10-minute window. At the poll's instant alone, the count falls to 1,256.
      - The counts are of the order the published figures imply. No source publishes how many
        trains run at once, so the comparison is per day:
        - In 2024, DB Regio ran 20,930 trains a day and DB Fernverkehr 1,518 [1].
        - DB ran 59.3% of the regional train-km ordered in 2024 [2]. Scaled by that share, every
          operator together runs about 35,000 regional trains a day. That figure includes S-Bahn
          trains, which the capture leaves out.
        - From 00:01 to 10:31 UTC, the capture saw 7,814 regional trips and 474 high-speed and
          long-distance trips.
      - No train is counted twice. At the peak poll, 1,384 regional trip ids carry 1,382 distinct
        pairs of agency and train number.
      - [1] Deutsche Bahn, *Daten & Fakten 2024*, "Züge/Tag",
        https://ibir.deutschebahn.com/2024/fileadmin/downloads/DB_DuF_d_2024.pdf
      - [2] Deutsche Bahn, *Wettbewerbskennzahlen 2024/25*, "Marktanteile im Regionalverkehr",
        https://www.deutschebahn.com/resource/blob/13583690/400096e0d9ac96425187a689c7a33a6e/Wettbewerbskennzahlen-2024_25-data.pdf

#### One train_segment version

Next after Motis capture. Bronze stays as written: `motis_segment` keeps its rows, and ingest
maps them into `train_segment_v2`.

- [x] Record two sources for the `motis_segment` captures, both the local server, feed `local`, and
      Motis v2.10.2: label `gtfs.de` for the captures of 19–22 July 2026, and label `delfi` for
      that of 25 July, after the server moved to DELFI on 23 July. Derive their legs into
      `train_segment_v2` under each.
      Ingest gives each `motis_segment` row its source by capture date, and deduplicates within
      each source. A binary committed and then removed recorded the two sources. Each July day
      derives the same number of legs as in `train_segment`: 1,361, 1,394, 1,835, and 588.
- [x] Measure `off_track_share` over the day's DE capture: the share of a leg's length lying more
      than 20 m from every rail segment in the country's Overture extract, sampled every 25 m.
      `notebooks/motis_capture/germany_delfi-2026-10-10.route-shapes.within.py` derives it. If any
      leg within Germany scores above 0, add it to `train_segment_v2`, with the extract's id beside
      it.
      Otherwise the poller's own filter, which keeps only legs within the country, is enough.
      - Measured over half a day, from 00:01 to 10:31 UTC on 18 October, which is enough to show
        the problem's size. Of 16,076 segments, 65 score above 0. They cover 114 km of 183,050 km,
        and 46 km of it lies off track. Every high-speed and long-distance segment scores 0.
      - 50 of the 58 segments scoring over 0.5 are on Chemnitz's tram-train lines, C11 to C15. The
        rest are RB25 near Saalfeld, the Kandertalbahn, and one RB69/RE7 leg at Augsburg.
      - A check of these segments on the map found each one follows a sensible real route.
      - Too few legs score above 0 to justify a column. The poller's filter is enough, and
        `train_segment_v2` stays as it is.
- [x] Remove `train_segment` and `TrainSegmentRowV1` once nothing reads them.

#### Ground truth

- [x] Record a pass per visit in `crates/session_crossings/src/matching.rs`. Test it on a session
      that reverses over a crossing.
- [x] Expose the matching's steps from `matching.rs`: the samples near a crossing, the visits, and
      the passes. Build `passes()` from the same steps, so the two cannot drift apart.
- [x] Add a bridge in `crates/session-crossings-py`, as `crates/platform/rerun-py` binds the
      predictor. It runs those steps from Python and returns each as an Arrow table. Test it with
      pytest, and run the tests under `just test-python`.
- [x] Add a geo test of real sessions to `session_crossings`, run by `just test-geo`. It reads named
      cases from a cases file. Each case names a session, a crossing, the passes expected, and why
      the case was chosen. The first case is a session that reverses over a crossing.
- [x] Add a notebook that draws those steps for each case, through the bridge. It draws the
      session's path, the crossing, the match radius, each visit's samples, and each pass.
- [-] Interpolate each pass's instant between the bracketing samples. Draw the bracketing samples
      and the interpolated point in the notebook too.
      Moot on 2026-10-19: the nearest sample is at most about 5 s off, at 10 s between samples.
      Every instant the capture records from Motis is a whole minute, realtime ones included.
- [x] Derive silver `train_session` and `train_session_sample` from bronze `motis_segment_v2`, in
      `motis_ingest` beside `train_segment_v2`. A train session is one trip, and its samples say
      where the train was every 10 s. Take each leg's times from its newest capture, as
      `train_segment_v2` does. Collapse a trip's legs by the stop each leaves from, not by its
      departure. A changed delay moves a leg's departure, so collapsing by departure keeps the old
      copy beside the new one. Of 452,521 pairs of consecutive legs captured by 19 October, 92,717
      overlap, and 68,214 of those leave from the same stop. Refuse a trip whose legs still
      overlap. Place the train along each leg's shape at constant speed, and at
      the stop between legs. Give both datasets the columns and partitions of `session` and
      `session_sample`, with `source_id` and `trip_id` in place of `device_id`. Leave out what
      only a device reports: `alt`, `acc`, `speed`, `heading`, `started_by`, `gap_seconds`, and
      `lead_seconds`. Name a train session by a UUID v5 of its source and trip. Mark each sample's
      `geometry_source` as `Stop`, at a leg's departure or arrival, or `Interpolated`. Later
      slices replay trains this way, at scale, to evaluate the predictor.
- [x] Make `train_session` readable by a filtered query, before the notebook below. A `WHERE` on
      a column other than the partition key panics in SedonaDB, at `spatial_filter.rs:526`,
      because `bbox` is a struct ahead of the geometry.
      [2026-10-03-sedona-nested-column-panic.md](2026-10-03-sedona-nested-column-panic.md)
      describes the panic, and the medallion README gives the two ways round it.
- [x] Draw one train's samples in a notebook, over its route, coloured by `geometry_source`.
- [-] Research how to represent a crossing's discernibility in space and time. Compare how closely
      a real train can revisit a crossing with how uncertain a session's samples leave the train.
      Look for prior art, such as scale space in images. Record the findings before changing the
      matching.
      Moved on 2026-10-19 to the slice "Evaluation framework with associated Crow Flies predictor
      improvement", which scores each pass, so a split pass scores twice.
- [x] Run `just silver-session-crossings` and `just gold-pack-sessions`, and move
      `sessions.version`.

#### Refactoring

At the end of the slice, once its features are in.

- [x] Look across the lookout crates for types a library or `std` already provides, as
      `TimeWindow` was a `Range<DateTime<Utc>>`, and for raw numbers standing in for a type.
      Replace each where the code reads better.
      A row type keeps `lat` and `lon` as `f64`, since each field mirrors a stored column. A
      distance stays an `f64` in metres, since the predictor's arithmetic reads better on raw
      numbers.
- [x] Name every flag, field, and type whose value is Overture's own after Overture, such as a
      release or a division's id and name. Check the other crates follow it. A country or region
      code keeps a plain name, since ISO defines it. Overture's schema types `country` as an ISO
      3166-1 alpha-2 code, and `region` as an ISO 3166-2 code. A type or dataset named after
      Overture, such as `OvertureRegion` or `overture_extract`, keeps plain field names. A stored
      column takes its new name in a new version of its bronze dataset, and the old version keeps
      its data:
      1. Rename `--release` on `extract new` to `--overture-release`, and `Area`'s `region` and
         `release` to `overture_region` and `overture_release`. Rename `Region`, which holds a
         GERS id, to `OvertureRegion`, and move it to `overture.rs`. Rename `transport`'s
         `RegionArea` to `OvertureRegionArea`, holding an `OvertureRegion` in place of its `id` and
         `name`. Rename `--overture-country` and `--overture-region` on `motis_track_area` to
         `--country` and `--region`. Name each code's standard in its flag's help, in
         `RegionCode`'s error, and in the docs.
      2. Add bronze `motis_source_v2`, with `region_id` and `region_name` renamed to
         `overture_region_id` and `overture_region_name`. Add bronze `extract_manifest_v2`, with
         `release` renamed to `overture_release`.
      3. Copy every row of `motis_source` and `extract_manifest` into its new version, with a
         one-off binary committed and then removed. Nothing writes either old version before step
         4 lands, so one run copies every row.
      4. Point every reader and writer at the new versions. These are `motis/src/source.rs`,
         `transport`, the `crossings` geo test, `medallion-py`'s bronze query test, the
         `motis_capture` and `water_crossings` notebooks, the comment in
         `data/medallion/.gitignore`, and the docs describing either dataset. Once nothing reads
         them, remove `MotisSourceRow` and `ExtractManifestRow`.
      5. Delete `motis_source` and `extract_manifest`, and remove both from the bronze registry.
         Their rows live on in the new versions, and their files in git history.
      Changed on 2026-10-20 from naming the country and region codes after Overture, as
      `--overture-country` and `--overture-region` did.
- [x] Name every column of silver `water_crossing` whose value is Overture's own after Overture:
      `water_id`, `water_subtype`, `water_class`, `rail_id`, and `rail_class`. Add
      `water_crossing_v2` with the new names, as `train_segment_v2` replaced `train_segment`.
      Derive it, point every reader at it, and remove `water_crossing` and `WaterCrossingRow` once
      nothing reads them.
- [x] Apply the `Age` rule in `.claude/rules/rust.md` across `lookout`: every maximum age,
      lookback, or other age held as `domain::Age`, every age flag read as a `Span`, and
      `Option<Age>` where no age applies.
      Outside `motis`, which already followed the rule, the ages were the recorder's gap and
      lead. `sessionise` takes them as `--gap` and `--lead`, in place of `--gap-mins` and
      `--lead-secs`. `Span` moved to `domain`, beside `Age`, here rather than in the next task,
      since the recorder does not depend on `motis`.
- [x] Take every duration, interval, and age a person writes across `lookout` as a human-readable
      `Span`, such as `30s` or `4h`, as the Motis pollers do. Move `Span` out of `crates/motis` to
      where every crate can reach it.
      The `Age` task did this. It moved `Span`, and converted `sessionise`'s `--gap-mins` and
      `--lead-secs`, the only duration flags left. No config value holds a duration, and every
      recipe already passes one as text, such as `10m`. The browser's and the device's timing
      constants stay, since no person writes them at run time.
- [x] Give each id in the ingest's `Leg`, in `crates/motis/src/ingest.rs`, a newtype in place of
      its `String`, as `TripId` is for `trip_id`. That covers `trip_id`, `from_stop_id`, and
      `agency_id`. Make `mode` an enum of the modes the capture keeps, in place of its `String`.
      Carry the new types into the rows built from a `Leg`. Drop `route_color` from `Leg` and
      `TrainSegmentRowV2`, since nothing reads it. Bronze keeps it, as the poll saw it.
      `RailMode` holds the six modes, and the capture's filter now reads it. The July captures
      hold 20 ferry legs, from 9 Rhine cruise ships on 21 July, a mode today's capture refuses.
      Ingest leaves out any leg of another mode and counts it as `not_rail`, so the next
      `silver-motis-ingest` drops those ferries from both silver datasets. A `Leg` checks its ids
      as it is read, so an empty id in bronze is an error.
- [x] Store `TrainSessionRow::source_label` as a `Label`, in place of its `String`. Tracing a
      row's schema deserialises an empty string into each field, and `Label` refuses one, so
      `medallion` needs a way to trace a checked newtype first. `MotisSourceRowV2::label` has the
      same `String` for the same reason.
      `Row::example` is that way: a row type that supplies an example is traced from it. The
      example holds `Some` in every `Option`. It holds the first variant of each enum whose
      variants carry no data, as `crates/medallion/README.md` describes.
- [x] Define `path_through` and `implied_speed` once, where `recorder` and `motis` can both
      reach them. `crates/recorder/src/silver.rs` and `crates/motis/src/train_sessions.rs` each
      define both.
- [x] Make `Radius::new` refuse anything but a positive, finite number of metres, as `Age`
      refuses zero. Refuse an invalid radius where it enters: `--match-radius-m` at parsing, and
      both Python bindings with a `ValueError`.
      The replay runner's `--radius-metres` refuses one at parsing too, through `radius_metres` in
      `lookout_predictor`.
- [x] Make every checked newtype check its value whenever it is deserialised, as `Label` does:
      1. Give every row type an example. Compare each example's trace with its type's trace, in a
         test committed and then removed.
      2. Make `Row::example` required, and trace from it alone. Update
         `crates/medallion/README.md` to match.
      3. Query the store for an empty id, since every read would then refuse one.
      4. Switch `TripId`, `StopId`, `AgencyId`, `SessionId`, `DeviceId`, and `CrossingId` from
         `#[serde(transparent)]` to `try_from = "String"`. Remove `parsed` and
         `parsed_if_present` from `crates/motis/src/ingest.rs`. Build for the device and for
         wasm, since `domain` holds three of the ids.
      5. Hold `MotisVersion` and `OvertureRelease` in `MotisSourceRowV2`, in place of their
         `String`s. `MotisSource`'s conversion then keeps only the check that a country and a
         release come together.
      On 23 October, no stored id broke its type's check, across the ten datasets that hold one.
- [x] Write the geometry columns first in every silver file, in `geo_batch` in
      `crates/medallion/src/geo.rs` and `Columns::batch` in `crates/medallion/src/table.rs`. A file
      whose geometry precedes its nested columns reads normally in a filtered SedonaDB scan. Then
      remove `holds_nested_columns`, `register_of_country_without_geometry`, the plain-parquet arm
      of `register_silver`, and the README's caveat about silver. Every silver dataset then returns
      typed geometry, where `session` and `train_session` return WKB bytes today.
      `notebooks/motis_capture/train_sessions.py` can then read its tables with
      `GeoDataFrame.from_arrow`. Re-derive silver afterwards.
- [x] In the "Dataset versions" section of `docs/medallion.md`, record that every change to a
      dataset's declaration makes a new version. That includes an added optional column and a
      widened type. Draw on
      `2026-10-24-breaking-dataset-changes.md`. State what
      a declaration covers: column order, types, nullability, the constraints a row type adds, and
      the spec. Include the changes no declaration shows, such as a column's meaning. A person
      judges whether one is significant, and sets a semantics tag that forces a new version.
      Correct where `docs/medallion.md` says a bronze file never changes and is never deleted.
      Bronze appends by default, and a lossless transformation can replace its files, in place or
      by migrating a version into the next. Git history keeps the files replaced.
      The semantics tag waits for the hash test, which builds it: `docs/medallion.md` describes
      only what holds now.
- [x] Apply those rules by hand to the changes this slice made to stored datasets. These include
      moving the geometry first, checking each id on every read, and dropping `route_color` from
      `train_segment_v2`. Add a task for each dataset that needs a new version.
      Six datasets on `main` changed their declarations in this slice, and each gets a new
      version. `session` and `session_sample` moved their geometry first. Those two,
      `session_crossing`, `gps_reading`, `accel_reading`, and `device_session` check their ids on
      every read. `session_crossing` also holds a row per pass. `raw_sample`, `motis_segment`, and
      `overture_extract` are unchanged. The datasets this slice created keep their names. No
      reader outside this branch saw an earlier declaration, and every stored file reads under
      the current one. That exception covers this slice's changes alone. Once the hash test
      lands, every change to a declaration makes a new version, on a new dataset as on any other.
- [x] Before any further change to a dataset, add an `insta` snapshot test of each dataset's
      declaration, in `crates/medallion-model`. A dataset's snapshot holds its name, layer,
      partition key, and a semantics tag, empty by default. It also holds the columns its files
      store, in order, its unique columns, and its enums' variants. Take the columns from one
      function the writers share, and the variants from `serde-reflection`. Mark each checked
      newtype's check with `#[hashfn]`, and keep every check's hash in one more snapshot. Run the
      test with `INSTA_UPDATE=unseen`, so a new version's snapshot is recorded on its first run. A
      changed snapshot goes to review, by a person or an agent. Review accepts a change readers
      cannot see, such as a refactored check, and otherwise makes a new version. Describe the
      review in `crates/medallion/README.md`, and the semantics tag in `docs/medallion.md`.
      Each column's checks sit in its dataset's snapshot rather than in one more snapshot. Every
      newtype a row holds now serializes under its own name, so the trace names each column's
      type. `TrainNumber`, `CrossingCompactId`, and `MotisSourceId` gained a `new` that accepts
      every value, so a check added to one later is caught. The table writer takes its geometry
      columns from `geometry_fields`, as the Rust writers do, and a test holds its layout to
      `file_schema`.
- [x] Give `session` a new version, `session_v2`. Point its writer and readers at it, re-derive
      it, and delete the old version's files.
      `notebooks/sessions` is deleted rather than moved to `session_v2`.
- [x] Give `session_sample` a new version, `session_sample_v2`, as for `session_v2`.
- [x] Give `session_crossing` a new version, `session_crossing_v2`, as for `session_v2`.
      Two passes named a crossing that the water crossings run of 24 October renamed.
      Re-deriving gives them its new id.
- [x] Give `gps_reading` a new version, `gps_reading_v2`. Copy its rows into the new version with
      a one-off binary, committed with the data it writes and then removed. Point its writer and
      readers at it, and delete the old version's files.
      The copy keeps each row's ingestion date. Each date's rows sit in one file named for the
      instant of the copy, since `Query` does not show which batch wrote a row.
- [x] Give `accel_reading` a new version, `accel_reading_v2`, as for `gps_reading_v2`.
- [x] Give `device_session` a new version, `device_session_v2`, as for `gps_reading_v2`.
- [x] Make `Leg::source_id`, in `crates/motis/src/ingest.rs`, a `MotisSourceId` rather than an
      `Option`. Both datasets `captures` reads give every leg a source: `motis_segment_v2` stores
      one in every row, and `legacy_source_id_sql` gives every `motis_segment` row one. Remove
      `IngestError::Unsourced` and its checks in `ingest.rs` and `train_sessions.rs`. Nothing else
      in `TryFrom<&Leg> for TrainSegmentRowV2` can fail, so make it a `From`. In the same file,
      remove `impl Add for IngestOutcome`, which nothing calls. Replace the `continue` in the
      `Shape::Missing` arm with a match that yields an `Option`, as `.claude/rules/rust.md` asks.
      `normalised`, in `train_sessions.rs`, already maps `Shape::Missing` to `None`. Remove
      `legacy_source` from `legacy.rs` too: only tests call it, and `legacy_source_id_sql` holds
      the same rule for production. The rule gives a legacy capture the `gtfs.de` source before
      the move to DELFI on 23 July, and `delfi` from then on. Three tests in `legacy.rs` check
      the rule in `legacy_source` alone. The one test that runs the SQL, in `ingest.rs`, compares
      sorted lists of source ids, which match even with the two sources swapped. Replace all four
      with two ingest tests, one on each side of the move. A legacy capture at 23:59:59 UTC
      on 22 July is derived under `gtfs.de`, and one at 00:00 UTC on 23 July under `delfi`. The
      rows `ingest` writes stay the same, so silver needs no re-deriving.
- [x] Hold an `OvertureRelease` wherever a release enters `transport`: the `--overture-release`
      flag, `ReleaseChoice`, `Release`, and `ExtractManifestRowV2::overture_release`. Pick the
      newest release by its order, and remove `release_order` in `crates/transport/src/overture.rs`,
      which parses the same text. Hold `ExtractManifestRowV2::country` as a `Country` too, giving
      `Country` the `try_from = "String"` serde the ids have. The `.parse()` calls and `Release`
      error variants in `countries.rs` and `regions.rs` then go. The two parsers disagree today:
      `release_order` takes `2026-09-23.01`, which `OvertureRelease` refuses.
      Checking the release changed the manifest's declaration, so it moved to
      `extract_manifest_v3`, copied by a one-off binary. The country stays text, as
      [docs/overture.md](overture.md#taking-an-extract-reads-the-country-division-filling-one-in-reads-the-manifest)
      requires: a row naming a country the store has no zone for still fills in.
- [x] Hold `base_url`, `country`, and `overture_region_id` in `MotisSourceRowV2` as `Url`,
      `Country`, and `GersId`. Reading a row then checks each, as it checks the version, release,
      and label. `Country` and `GersId` need the `try_from = "String"` serde the ids have, and `url`
      its `serde` feature. If nothing outside tests then converts a row back into a `MotisSource`,
      remove `TryFrom<MotisSourceRowV2> for MotisSource`, `NotAMotisSource`, and their tests.
      The checked columns changed the source's declaration, so it moved to `motis_source_v3`,
      copied by a one-off binary. The country stays text, as
      [docs/motis.md](motis.md#a-capture-names-the-server-that-answered-it) states. The base URL
      is a `MotisBaseUrl` rather than a bare `Url`: `url`'s serde writes a plain string, which
      hides the check from the declaration snapshot.
- [x] Find a country's newest extract once, in `CountryAreas` in
      `crates/transport/src/countries.rs`. Pass its id to `region_area` in `regions.rs`, which
      repeats the lookup today. Then remove `OvertureRegionArea::release`, `RegionError::NoExtract`,
      and `RegionError::Release`, and let `fixed_area` in `motis_track_area` read the manifest once.
      Share the test helpers `regions.rs` copies from `countries.rs`, such as `square_from` and
      `division_area`.
      `RegionError::Release` went with the release's parse, in the `OvertureRelease` task. Each
      test module keeps its own `division_area`, since the two build different columns. Recording
      an extract is shared instead.
- [x] Give `captures`, in `crates/motis/src/ingest.rs`, one leg identity that survives a delay:
      source, trip, departure stop, and scheduled departure. It replaces the `keyed_by` SQL each
      caller passes. `ingest` keys `train_segment_v2` on the realtime departure today, so it keeps a
      delayed leg once for each departure it saw. `train_session` keys on the departure stop alone,
      merging them, and counts nothing. Count each dataset's legs before and after, and update "A
      leg's identity" in `docs/motis.md`.
      Over the captures to 25 October, the old keys kept 1,808,403 rail legs for the segments and
      1,702,958 for train sessions. The new key keeps 1,702,983 for both. It keeps 97,685 delayed
      legs once rather than once per departure, and splits 25 trip and stop pairs that have two
      scheduled departures. The new grain made the segments `train_segment_v3`, with its semantics
      tag set. `motis_ingest` wrote 1,702,978 rows to it, and 165,606 train sessions.
- [x] Let `Sorted::Kept`, in `crates/motis/src/capture.rs`, carry the trip id, line, and segment
      that `sorted` has checked. `SegmentLog::append` in `bronze.rs` then cannot fail on them. The
      same check runs twice today, and its second copy, `BronzeError::Segment`, loses a whole poll
      where the first skips one segment and reports it.
      `Sorted::Kept` carries a `SingleTripSegment`: the segment, its one trip, and that trip's id.
      It leaves out the decoded line, since nothing after the coverage test reads it.
- [-] Fold the two side-by-side notebooks in `notebooks/motis_capture` into one that takes its
      pass's label. It can read the label from `mo.cli_args()` or a dropdown of `motis_source_v2`
      labels. They differ in 44 of about 920 lines. First move each pass's findings into the slice
      record, since each notebook records its own. Then move the steps
      `compare_delfi-2026-10-10.plain_route-shapes.py` repeats into a module of the `motis_capture`
      project. Move there the `bronze()` and `shape_of()` helpers four notebooks define. Do this
      before the Six-day capture points the notebooks at `motis_segment_v3`.
      Moot on 2026-10-27: the analysis is done, and `notebooks/motis_capture` is deleted.
      Observations holds the baseline pass's findings and the comparison's.
- [-] In `notebooks/motis_capture/germany_delfi-2026-10-10.route-shapes.within.py`, build the
      segment table and its metric shapes in one cell, and the bar charts' styling in one helper.
      Two cells run the same query and filter today, and the same polylines are decoded and
      projected three times. Decode only distinct polylines: about 22,000 of the 260,000 rows'
      polylines differ.
      Moot on 2026-10-27: the notebook is deleted, with the rest of `notebooks/motis_capture`.
- [-] Read the store through `lookout_medallion` in `notebooks/motis_capture/v1.py`, as its
      siblings do and `.claude/rules/lookout-notebooks.md` requires. Use `default_root()` in place
      of `medallion_root()`, and `query_bronze` in place of `read_parquet` over hand-written paths.
      Moot on 2026-10-27: the notebook is deleted, with the rest of `notebooks/motis_capture`.
- [x] If lonboard 0.16.0 takes hex colours in `get_color` and `apply_categorical_cmap`, pass them
      through unparsed. `session_crossings/passes.py` parses them by hand.
- [x] Derive `Feed`'s names, parsing, and list with strum, as `RailMode` does, in
      `crates/medallion-model/src/motis_source.rs`. They replace its hand-written `ALL`, `as_str`,
      `names`, and `FromStr`. Keep `UnknownFeed`, listing the names from `Feed::iter()`.
- [x] Move the fixtures `crates/motis`'s unit tests each define into one `#[cfg(test)]` module, as
      `medallion-model`'s `testing.rs` holds its column helpers. These are the local `MotisSource`,
      the `trips.json` loader, `Everywhere`, `Nowhere`, and `germany()`, defined in up to six
      modules each. `tests/common/mod.rs` keeps its own, since an integration test cannot reach a
      unit test module.
- [x] Simplify `crates/motis/src/sampling.rs`. Have `position_at` return a point, since nothing
      outside tests tells `Travelling` from `WaitingAtStation`. Build each `Placed` directly, in
      place of `(t, position, source)` tuples. Drop `Leg::duration`, which `arrival` minus
      `departure` gives, and have `Leg::new` check the arrival against the departure directly.
- [x] In `steps`, in `crates/session_crossings/src/matching.rs`, pair each sample with its distance
      once and group the pairs into visits once. `steps` groups, flattens, and groups again by visit
      index today, and computes each distance three times.
      Extended on 2026-10-30: `domain`'s `Pass::new` now builds each pass and refuses a sample
      count too big for a `u32`, which the old code clamped. An empty visit, which `pass_of`
      dropped, is now an error. `steps` and `passes` return both errors.
- [x] Build `session-crossings-py`'s output tables with `serde_arrow::to_record_batch` on row
      structs, as `medallion::batch` does. They replace its `batch` helper and the column lists
      `src/lib.rs` keeps by hand.
- [x] Give `SessionId` a `From<Uuid>`, as `DeviceId` has, and make the train session namespace a
      constant, as `SESSION_NAMESPACE` is. `train_session_id` then needs no `expect`, and hashes no
      namespace per call.
      Changed on 2026-10-30: the namespace stays derived from its name in `train_session_id`, since
      the derivation says where it comes from and a UUID literal does not. Only the `expect` goes.
- [x] Derive `sessionise`'s `--gap` and `--lead` defaults from `DEFAULT_GAP` and `DEFAULT_LEAD` with
      `default_value_t`, as `match_crossings` does from `DEFAULT_RADIUS`. They replace the literals
      `"10m"` and `"1m"`, and the test keeping them in step. This needs a way to turn an `Age` into
      a `Span`.
- [x] If `nutype` builds for Xtensa and wasm, generate the non-empty id newtypes with it: `TripId`,
      `StopId`, `AgencyId`, and `DeviceId`. Each writes out `new`, `FromStr`, `TryFrom<String>`,
      `From<_> for String`, and an `Empty*` error by hand.
      Extended on 2026-10-30: each check is a `#[hashfn]` validator `nutype` calls, so the
      declarations still fingerprint it. The declaration snapshots changed only in those hashes. A
      property test shows each check still accepts exactly the non-empty texts, so the change is
      not breaking, and the snapshots are accepted.
- [x] Register each dataset under its own name, as the "Dataset versions" section of
      `docs/medallion.md` states. In production, `crates/recorder/src/sessions.rs` registers
      `gps_reading_v2` as `samples` and `device_session_v2` as `session_starts`. In tests,
      `crates/motis/src/ingest.rs` registers `train_segment_v2` as `derived`, and
      `crates/motis/src/train_sessions.rs` and the `recorder` crate use `sessions`, `samples`,
      and `d`. Leave the test setup in `crates/motis/tests/common/mod.rs`, which registers
      `motis_segment_v2` as `captured`, to the Six-day capture's move to `motis_segment_v3`.
- [x] Remove the semantics tag from the code. A significant change of meaning still makes a new
      version, and the version in a dataset's name already marks that change. The tag's only input
      is the decision it reports, and only the declaration test reads it.
      1. Remove `Row::SEMANTICS` from `crates/medallion/src/rows.rs`, its one value on
         `TrainSegmentRowV3` in `crates/medallion-model/src/motis.rs`, and the `semantics` field of
         `Declaration` in `crates/medallion-model/src/declaration.rs`.
      2. Accept the 16 declaration snapshots, which each lose one line, with no new versions. No
         reader can see the change.
      3. In the "Dataset versions" section of `docs/medallion.md`, remove the semantics tag from
         what a declaration covers. Replace the paragraph on a change of meaning with:

         > A change of meaning leaves the declaration as it was. Such changes include a new unit,
         > another value standing for "none applies", a new grain, and a row identity that varies
         > between runs. The declaration test cannot find one. Where a person judges one
         > significant, they give the dataset a new version, as for a change to its declaration.

      4. In "Each dataset's declaration is snapshotted", in `crates/medallion/README.md`, remove
         the semantics tag from what a snapshot holds. Rewrite the paragraph on a change of meaning
         to match.
      5. Reword the Six-day capture task that sets a semantics tag, so it gives each dataset a new
         version directly. Notes on completed tasks stay as history.
- [x] Check that everything of value in
      `2026-10-24-breaking-dataset-changes.md` has been
      applied or recorded. Record what has not, in `docs/medallion.md` or
      `crates/medallion/README.md`. Then delete the research doc.
- [ ] Replace `legacy` in `crates/motis` with the name of the dataset it stands for,
      `motis_segment`. `append_legacy` becomes `append_to_motis_segment`, `ingest_legacy_one`
      becomes `ingest_one_from_motis_segment`, `legacy_sources` becomes `motis_segment_sources`,
      and `legacy_source_id_sql` becomes `motis_segment_source_id_sql`. `legacy.rs` becomes
      `motis_segment_sources.rs`. Apply the same change to five test names, two `.expect` messages,
      and a local variable in `captures`. If the next task runs first, this one is moot, since that
      task deletes all of these names.
- [ ] Migrate `motis_segment` into `motis_segment_v3` and `motis_polyline_v2`, then delete
      `motis_segment` and the code only it needs. This reverses One train_segment version's choice
      that `motis_segment` keeps its rows. The dataset holds 60,243 rows in 1,307 files, captured on
      four days from 19 to 25 July 2026, and 236 of those rows are ferries. No polyline is empty or
      holds a character outside 63 to 126, so only a failure to decode can refuse one.
      `motis_source_v4` already holds both of its sources.
      1. Write a one-off binary that reads the rows through `Query` and writes them through the
         drain's `write_polls`. Map each row with `MotisPollRowV2::of`. Its source follows its
         capture date: `gtfs.de` before 23 July, and `delfi` from then on. If `MotisPollRowV2::of`
         refuses a polyline, stop and report the row. Commit the binary with the data it writes.
      2. Check the migration is lossless. `motis_segment_v3` joined to `motis_polyline_v2` must hold
         every `motis_segment` row, with only the source id added. Record the row counts under
         Observations.
      3. Check `just silver-motis-ingest` reports the same counts and derives the same
         `train_segment_v4` and `train_session_v2` rows as before the migration. A session whose
         legs share a departure can still differ in geometry, because such legs keep the query's
         order. Observations records this for `motis_segment_v2`.
      4. Pause so the user can inspect the result.
      5. Delete the binary, `motis_segment`'s files, `MOTIS_SEGMENT`, `MotisSegmentRow`, its
         declaration snapshot, and the module holding the rule for its sources. `segment_row`, in
         `crates/motis/src/landing.rs`, then builds a `MotisPollRowV2` directly. The two sources
         stay in `motis_source_v4`, since the migrated rows name them.
      6. Reduce `captures`, in `crates/motis/src/ingest.rs`, to one dataset. `Captures`,
         `POLYLINE_IN_THE_ROW`, and `captured`'s parameters go. `ingest` then needs only one of its
         two sorts by departure: the query's `ORDER BY` and `legs.sort_by_key`.
      7. Make `Leg::polyline` a `Polyline`, since `motis_polyline_v2` holds only checked text.
         `shape_of` then cannot fail, so `decode_polyline`, `NotAPolyline`, and
         `IngestError::Polyline` go.
      8. Rewrite the tests that write `MotisSegmentRow` so they write polls. Delete those that need
         an empty or unreadable polyline, the rule for the sources, or both datasets. Keep the
         `Shape::Missing` case, with a polyline that repeats one point.
      9. Update `docs/motis.md`, `crates/medallion-model/README.md`, and the Evaluation framework
         slice in `docs/next-slices.md`, which all name `motis_segment`.
- [ ] Make `Panel::new`, in `crates/platform/m5/m5plus/src/panel.rs`, leave its display unchanged,
      as `.claude/rules/rust.md` asks. It takes `mut display` today to clear the screen. Move the
      clearing into a `Panel::clear`, which `src/bin/m5plus.rs` calls straight after `new`. That
      keeps the screen black when the backlight comes on. `new` then returns a `Panel` rather than
      a `Result`. The user flashes the device to check the screen comes up black.

#### Six-day capture

Before the branch merges, so `main` never holds a poll file. Before Optimisation, since the
polyline table changes what `motis_ingest` reads. The namespaces are derived before
`motis_segment_v3` exists, so its rows hold the new source ids from the start.

- [x] Finish the six-day capture of Germany, which started on 19 October and ends around 25
      October. Commit its polls as they accumulate, and its last ones once the poller stops. Then
      run `just silver-motis-ingest`, so the train sessions cover the whole week.
- [x] Give `writer_at`, in `crates/medallion/src/write.rs`, zstd compression, for bronze and
      silver alike.
- [x] Derive `SESSION_NAMESPACE` and `MOTIS_SOURCE_NAMESPACE` from their names with UUID v5, as
      `train_session_id` derives its namespace. Every session id, source id, and train session id
      then changes.
- [x] Give each existing dataset holding one of those ids a new version. These are bronze
      `motis_source_v4`, and silver `session_v3`, `session_sample_v3`, `session_crossing_v3`,
      `train_segment_v4`, `train_session_v2`, and `train_session_sample_v2`.
      Each silver version replaces its declaration in place, since no code reads the old one.
      `motis_source_v3` keeps its declaration for the migration that reads it.
- [x] Migrate bronze `motis_source_v3` into `motis_source_v4` with a one-off binary. Compute each
      new source id from its row's fields. Refuse a row whose old id differs from those fields
      under the old namespace. Read and write through medallion's `Query` and writers. Commit the
      binary with the data it writes, and remove it in a later commit.
- [x] Add bronze `motis_polyline_v1` and `motis_segment_v3` to `crates/medallion-model`, with a
      `PolylineId` newtype holding the UUID v5 of a polyline's text. If serde_arrow traces a
      flattened row, compose the new row from shared parts with `#[serde(flatten)]`. That avoids
      restating every field, as `MotisCaptureRow` restates `MotisSegmentRow`'s. serde_arrow traces a
      flattened row, but the declaration test cannot. serde-reflection sees a flattened row as a
      map, and refuses a map whose values differ in type. `MotisSegmentRowV3` therefore restates the
      fields. A `Polyline` newtype holds the encoded text, and `PolylineId::of` takes one. It
      refuses empty text and any character outside ASCII 63–126, without decoding, since reading a
      row runs the check.
- [x] Add a landing dataset for the capture's polls, on `layers::Landing`, which no dataset uses
      yet. Give `medallion` a way to list its files and delete one once drained. Ignore
      `/landing/` in `data/medallion/.gitignore`.
- [x] Point `SegmentLog`, in `crates/motis/src/bronze.rs`, at the landing dataset, and rename the
      `bronze-poll-*` recipes `landing-poll-*`. The module is now `crates/motis/src/landing.rs`.
      `common/mod.rs` reads `motis_poll_v1`, since the poll tests read what a capture wrote and
      nothing drains there. Tests seeding `captures` write `motis_segment_v2` through a test-only
      `CaptureLog`.
- [x] Add `motis_promote_landing_to_bronze` to `crates/motis/src/bin/`, and a recipe
      `bronze-promote-motis-landing`, draining landing into `motis_polyline_v1` and
      `motis_segment_v3`.
- [x] Read `motis_segment_v3` in `captures`, in `crates/motis/src/ingest.rs`, joining
      `motis_polyline_v1` after picking each leg's newest capture.
- [x] Migrate `motis_segment_v2` into `motis_segment_v3` and `motis_polyline_v1` with a one-off
      binary, writing through the drain's code. Give each segment its source's id in
      `motis_source_v4`, and refuse a segment whose source is missing from `motis_source_v3`. Commit
      the binary with the data it writes, and remove it in a later commit.
- [x] Check both migrations are lossless. `motis_source_v4` and `motis_segment_v3` must hold every
      observation in the datasets they replace, with only the source id changed. Each old source id
      must map to exactly one new one. Record the row counts under Observations.
- [x] Record, under Observations, the sizes of the migrated files against the research doc's
      DuckDB figures.
- [x] Check `just silver-motis-ingest` derives the same `train_segment_v4` and `train_session_v2`
      rows from `motis_segment_v3` alone as from `motis_segment_v2` alone, but for the source ids
      and the train session ids derived from them. The `motis_segment_v2` side is the silver of the
      30 October rebuild, since current code cannot read its old source ids. 34 sessions differ
      through legs tied on departure, as Observations records. The slice "replay the trains whose
      legs overlap" now holds that fix.
- [x] Rederive the recorded sessions into their new versions with `just silver-sessionise` and
      `just silver-session-crossings`. Check each holds its old version's rows, with only the
      session ids changed. Then run `just gold-pack-sessions`.
- [x] Update the session ids held outside the store: in
      `crates/session_crossings/tests/passes.geojson`, and in any test or notebook that names one.
- [x] Delete `motis_segment_v2`'s files, `MOTIS_SEGMENT_V2`, its entry in
      `crates/medallion-model/src/bronze.rs`, and its reading in `captures`. Its rows live on in
      `motis_segment_v3`.
- [x] Tell a leg with no polyline from a leg with an invalid polyline, in `categorise`, in
      `crates/motis/src/capture.rs`, and give each case its own count and log line.
      - No polyline: Motis sends empty text. Decide whether to keep such a leg. `categorise` decodes
        it to an empty line and counts it as outside the coverage, so it lands nowhere today. If the
        leg stays, `motis_poll_v1` and `motis_polyline_v1` must hold it, since `Polyline` refuses
        empty text.
      - Invalid polyline: text the decoder refuses, or with a character outside ASCII 63–126. Ignore
        that leg's row, and land the poll's other rows. `categorise` does this today for a leg the
        decoder refuses, but counts it with the other unresolved legs. A character above 126 can
        pass the decoder, and `SegmentLog::append` then fails the capture and loses the poll's other
        rows.

      Decided on 2026-10-31: a leg with no polyline stays out of landing, since the coverage test
      needs its line. The store cannot show how often Motis sends one, because captures have always
      left these legs out, counted as outside. The new count will show it. `SegmentLog::append`
      still refuses an invalid polyline, for a caller other than `capture`.
      Extended on 2026-10-31: building a `Polyline` now decodes it as well as checking its
      characters, so `TripSegment::line` needs no check of its own. That changes the declarations of
      `motis_poll_v1` and `motis_polyline_v1`, which become `motis_poll_v2` and `motis_polyline_v2`.
      `motis_ingest` had already decoded every polyline the store holds.
- [x] Once the user has inspected the migrations, delete the files and declarations of
      `motis_source_v3` and of the silver versions the new ones replace.
- [x] Describe landing, the drain, `motis_polyline_v2`, and `motis_segment_v3` in
      `docs/medallion.md` and `docs/motis.md`, in place of `motis_segment_v2`, and name the other
      new versions there. A third-party service's samples take one bronze file per drain. Then fold
      what holds of `2026-10-23-compact-bronze.md` into them, and delete it.
- [x] Rewrite this branch's commits since it left `main` without the `motis_segment_v2` files,
      using `git filter-repo` in a fresh clone. Before opening the PR, force-push the result.

#### Optimisation

After Refactoring, once the slice works end to end.

- [ ] Cut the memory `motis_ingest` takes. On the captures to 25 October, it peaks at a footprint
      of 4.26 GB and writes 64.2 million train session samples in 216 s. `motis_ingest` on the
      captures to 25 October, under Observations, profiles it by phase. Three costs are suspected.
      Every collapsed leg is held twice, once for segments and once for sessions. Each session's
      path runs through every sample, held in degrees and projected. Each open partition file
      buffers a row group of up to a million rows. A path from the legs' shapes would also shrink
      `train_session_v2`, at 1.52 GB on disk.
      The peak now falls early in sampling, while most trips' shapes are still held. The
      segment phase stays under 3 GB, since its legs go in batches and each day's file closes once
      written. The third suspect holds only for the sample files, which stay open until sampling
      ends, and closing them frees 0.65 GB. The first costs time rather than memory, since nothing
      survives between the two derivations. The second holds in the session phase, which now sets
      the peak.
- [x] Flush each open partition file's row group as `GeoRowWriter` writes it, in
      `crates/medallion/src/derive.rs`, or close each file once its rows are written. Set
      `max_row_group_size` in `writer_at` too. Every (country, date) file buffers its compressed
      pages until `finish` today. A day of segments, at up to 263,917 rows, never fills the default
      row group of 1,048,576 rows.
      Decided on 2026-11-01: `write_geo_rows` closes each day's file once the day is written, which
      cuts the peak from 15.55 GB to 9.91 GB. A flush after each write reached only 13.46 GB and
      grew the sample files by 14%, so `GeoRowWriter::write` does not flush. `max_row_group_size`
      keeps its default. A smaller row group grows the files in the same way, and the writer
      encodes a whole batch before it drains any of it.
- [x] Write `ingest`'s legs to `train_segment_v4` in batches, through `GeoRowWriter::new`, as
      `derive_train_sessions` writes its samples. Today it collects all 1.70 million placed legs
      first. Iterate the captured legs by value, with a `From<Leg>` that moves their strings.
      `GeoRowWriter::write` keeps each file open until `finish`, so batching must also close each
      day's file once its legs end. About 5.6 GB of placed legs and 1.5 GB of bronze legs stay held
      through the write today.
      Extended on 2026-11-01: `GeoRowWriter::close_days_before` closes the days a sorted stream has
      passed, and the writer refuses a later row for a closed day. `ingest` sorts both captures'
      legs together, since one day can span them. This cuts the peak from 9.91 GB to 7.31 GB.
- [ ] Write a day's rows without copying them, in `geo_day` in `crates/medallion/src/derive.rs` and
      `wkb_column` in `geo.rs`. Serialise `&R`, encode WKB from `&G`, and project and encode one
      geometry at a time into a `BinaryBuilder`. A day's geometry exists about five times over at
      once today, 0.83 GB for the largest day of `train_session_v2`. Batches already bound
      `train_segment_v4`'s copies.
- [x] Compute each leg's cumulative distances once in `sampled`, in `crates/motis/src/sampling.rs`,
      and interpolate within the one segment `partition_point` finds. `point_at_ratio_from_start`
      measures and walks the whole shape again for every sample: about 30 billion Haversine
      evaluations for 54 million travelling samples.
      Decided on 2026-11-01: `sampled` holds the distances only while it samples a trip, since
      keeping one per vertex would add about 1.65 GB. This cuts the run from 935 s to 372 s.
      Positions now differ from the whole-shape walk by rounding, at most 1.1e-13 degrees, in 0.6%
      of samples.
- [x] Memoise `CountryAreas::containing`, in `crates/transport/src/countries.rs`, by point, or test
      a bounding box first. Each call walks all 160,000 vertices of Germany's outline, for 1.70
      million legs that start from 11,266 distinct points.
      Decided on 2026-11-01: `MemoisedCountries`, in `crates/motis/src/placement.rs`, holds the memo
      for one derivation, so `CountryAreas` stays plain data. A bounding box does not help, since
      nearly every point lies in Germany. This cuts the run from 372 s to 216 s.
- [ ] Project each geometry once. Let a `GeoRow` carry a projected geometry its caller already has.
      The writer then reuses `session_of`'s projection of every sample, and `ingest` projects each
      distinct polyline once. Of the segment projections, 98% repeat a shape already projected.
      Share one per-country projector cache between `GeoRowWriter::write` and
      `derive_train_sessions`, which copies it.
- [x] Release each trip's shapes once it is sampled. Iterate `trips` by value in
      `derive_train_sessions`, and pass `NormalisedLegs::new` its legs by value, in place of a deep
      copy of every shape. About 3.3 GB stays held through the session write today, and as much
      again is copied.
      Decided on 2026-11-01: `derive_train_sessions` iterates `trips` by value, which cuts the peak
      from 7.31 GB to 4.76 GB. `NormalisedLegs::new` keeps its copy. Passing the legs by value
      reached only 5.91 GB, because each decoded shape keeps the spare capacity of its growing
      `Vec`. The copy allocates the exact size and takes about 0.2 s in all. `shrink_to_fit` leaves
      the footprint unchanged on macOS.
- [ ] Let `captures` skip what `derive_train_sessions` discards: both count queries and the final
      `ORDER BY departure`. The session phase reruns the whole capture read today, carrying every
      leg's polyline text, 0.73 GB of it. `motis_polyline_v2` takes the text out of the
      `ROW_NUMBER` sort over `motis_segment_v3`. The query joins the text in before
      `ORDER BY departure`. `ingest` now sorts its own legs, so neither caller needs that sort for
      its order by day. Legs sharing a departure and arrival keep the query's order in
      `NormalisedLegs::new`, so removing it needs a tie-break there.
- [ ] When `motis_track_area` captures a whole country, test each leg once, with `covers`, against
      one prepared outline. Share the coverage, rather than cloning it each poll. It now passes the
      same shapes as area and as country, so it unions and prepares them twice and tests each leg
      twice.

#### Wrap-up

- [ ] Add the Motis source to the derivation graph in `docs/architecture.md`.
- [ ] Fold what holds of `2026-10-12-track-shapes-and-realtime.md` into `docs/motis.md`: the
      `route_shapes` setup, the train-name script, realtime resolution, and the measured import.
      Correct the `displayName` claim there. Then delete the research doc.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.

### Investigation

#### A pass split by imprecise samples

Re-deriving `session_crossing` with a pass per visit, on 2026-10-18, gave two sessions passing one
crossing twice. Session `a489b094` reverses at Frankfurt (Main) Hbf, a terminus, and crosses the
Main out and back, 13 minutes apart. Its samples report 2–5 m of accuracy, and it is the first geo
test case.

Session `71b9c047`, on 26 July near (50.355, 11.023), passes four crossings twice each, 22 s apart.
Its samples report about 1,450 m of accuracy. Their distance from one crossing runs 6,682 m, 234 m,
588 m, 155 m, and 2,873 m. The middle sample falls outside the 250 m radius, so one approach splits
into two visits. Samples that imprecise cannot show whether the train passed the crossing at all.

The visit rule cannot tell such jitter from a real revisit. A real revisit needs the train to leave
and return, which takes a span of space and time. Where a session's uncertainty exceeds that span,
two visits are no longer discernible. A representation of this in space and time, akin to scale
space in images, is a research task in the slice "Evaluation framework with associated Crow Flies
predictor improvement".

### Observations

#### The migrations of `motis_source_v3` and `motis_segment_v2`

On 31 October, DuckDB compared each migrated dataset with the one it replaces, using `EXCEPT ALL` in
both directions. Each comparison found no row on either side alone.

| Old | Rows | New | Rows |
| --- | --- | --- | --- |
| `motis_source_v3` | 8 | `motis_source_v4` | 8 |
| `motis_segment_v2` | 2,682,070 | `motis_segment_v3` joined to `motis_polyline_v1` | 2,682,070 |

- Source rows match on every column but `source_id`. The 8 old ids map to 8 new ids, one to one.
- Segment rows match on every column, including `polled_date` and the polyline text, once each old
  source id is replaced by its new one. Each date holds the same row count in both datasets.
- `motis_polyline_v1` holds 25,464 rows, each with a distinct id and distinct text. Every id is
  the UUID v5 of its text, and every segment's polyline id is present.

#### `motis_ingest` from `motis_segment_v3` against `motis_segment_v2`

On 31 October, `motis_ingest` ran in a scratch store holding `motis_segment` and `motis_segment_v3`,
without `motis_segment_v2`. Its output was compared, per partition and with source ids mapped, with
the silver the 30 October rebuild derived from `motis_segment` and `motis_segment_v2`.

| Dataset | Rows, each side | Partitions differing |
| --- | --- | --- |
| `train_segment_v4` | 1,702,978 | 0 of 16 |
| `train_session_v2` | 165,606 | 3 of 16 |
| `train_session_sample_v2` | 64,232,436 | 3 of 17 |

- Every train session id is the UUID v5 of its new source id and trip id.
- 34 sessions differ, all from one source, on 18–20 October. Their geometry differs, and 19 also
  differ in `bbox`. Every other session column matches. 1,566 of their 16,156 samples moved, by up
  to about 68 km. Their timestamps and row counts match.
- Each of those 34 trips has legs sharing a departure, such as two zero-duration legs at one
  instant. `Legs::new` sorts by departure and arrival, so such legs keep the order the query
  returned them in, and `captures` orders only by departure. The train segments match, so the
  difference comes from that order rather than from the migrated rows. Over those dates, 2,649
  departures of a trip are shared by two or more of its legs.
- The run read 2,742,313 rows and took 896 s, with a peak memory footprint of 15.6 GB and a peak
  resident set of 9.4 GB. The 12.7 GB under the captures to 22 October covers 1,975,310 rows, so
  the two do not show what the polyline table saves.

#### The sizes of the migrated files

parquet-rs writes about what the research doc measured with DuckDB, which settles that doc's open
question about the sizes. Every size sums file lengths. The research doc's figures cover
20–22 October, so this table does too.

| Over 20–22 October | Research doc, DuckDB | Migrated, parquet-rs 57.3.1 |
| --- | --- | --- |
| A file per day, polyline stored once | 5.4 MB a day | 5.65 MB a day |
| Polyline table, each day's growth | 0.1–0.4 MB of text | 0.10–0.27 MB of parquet |

- `motis_segment_v3` holds 39.1 MB and `motis_polyline_v1` 10.2 MB, 49.3 MB in all, against
  2,135.8 MB for `motis_segment_v2`. Those totals cover all 12 dates.
- `motis_segment_v2` takes 304.6 MB a day over 20–22 October on disk, so `motis_segment_v3` is
  about 54 times smaller there. The research doc's 58–70 MB a day measures those files in git,
  which finds deltas between them.
- The polyline table's first full day, 18 October, takes 6.46 MB of its 10.2 MB. The research doc's
  table takes 10.4 MB for the polylines 20–22 October saw. The migrated table holds 25,464 polylines
  from all 12 dates, against the research doc's 24,790 from its shorter span.
- The migration wrote a polyline file per date. A promotion writes one a run.

#### `motis_ingest` on the captures to 25 October

On 26 October, `motis_ingest` read 2,742,313 captured rows, which collapse to 1,703,003 legs.

- A run that started with about 13 GiB of disk free ran out of space while writing
  `train_segment_v3`. With 28 GiB free, the next run finished.
- A run on 1 November took 880 s and wrote 64.2 million train session samples. `/usr/bin/time -l`
  reported a peak memory footprint of 15.55 GB and a peak resident set of 10.28 GB. The run used
  927 s of CPU, about one core on average.

`footprint --sample 5` recorded how that run's footprint moved:

| Elapsed | Work | Footprint |
| --- | --- | --- |
| 0–10 s | Read the country areas and the captures | Up to 2.06 GB, then 1.48 GB |
| 10–144 s | Decode and place the legs | 1.48 GB to 7.10 GB |
| 144–221 s | Encode each day into its open file | 7.10 GB to 15.55 GB |
| 221–229 s | Close the 16 `train_segment_v4` files | Down to 1.14 GB |
| 229–240 s | Read the captures again | 3.51 GB |
| 240–856 s | Normalise and sample the trips, and write the samples | 3.51 GB to 6.05 GB |
| 856–880 s | Write `train_session_v2` | 6.05 GB to at least 8.60 GB |

- The log and the files' timestamps fix 221 s, 229 s, 856 s, and 880 s. The curve's shape and the
  code place 10 s, 144 s, and 240 s.
- No `train_segment_v4` file reached the disk before 221 s, and all 16 appeared within 8 s as the
  footprint fell. The open files hold their pages in memory until `finish`.
- The write's 8.45 GB holds 1.84 GB of `geo_day`'s copies of 24 October, the last large day, and
  6.61 GB left over. The files before that day take 4.25 GB compressed and 6.17 GB uncompressed.
  parquet-rs buffers compressed pages, so the other 2.36 GB is unexplained.
- These legs hold 219.9 million vertices in 26,965 distinct shapes, and start from 11,266 distinct
  points. Their polyline text takes 0.73 GB, against 1.86 GB before the rows of `motis_segment_v3`
  collapse.
- The 165,606 sessions' legs hold 206.7 million vertices, 3.31 GB of coordinates. The footprint
  sits at 3.51 GB as sampling starts.
- Each of the 54.4 million travelling samples measures its leg's whole shape and walks half of it
  on average. That comes to about 30 billion Haversine evaluations.
- Swap grew by 7.4 GB from 8.2 GB, on a machine with 24 GB. `time -l` printed 0 swaps, so that
  field does not show swap here. The swap files took 6 GiB of disk and kept it after the run.

Seven changes were measured on the same capture. The flush and the legs passed by value were
dropped, and each other change builds on those before it:

| Code | Peak footprint | Peak resident set | Time | Swap growth | Sample files |
| --- | --- | --- | --- | --- | --- |
| Each file open until `finish` | 15.55 GB | 10.28 GB | 880 s | 7.4 GB | 2.66 GB, 74 row groups |
| A flush after each write | 13.46 GB | 8.50 GB | 893 s | 3.5 GB | 3.04 GB, 735 row groups |
| Each day's file closed once written | 9.91 GB | 8.53 GB | 871 s | None | 2.66 GB, 74 row groups |
| Legs written in batches as well | 7.31 GB | 4.87 GB | 869 s | None | 2.66 GB, 74 row groups |
| Trips released, legs passed by value | 5.91 GB | 4.46 GB | 886 s | 1.8 GB | 2.66 GB, 74 row groups |
| Trips released, each shape copied | 4.76 GB | 4.47 GB | 935 s | None | 2.66 GB, 74 row groups |
| Each leg's distances computed once | 4.68 GB | 4.17 GB | 372 s | None | 2.66 GB, 74 row groups |
| Country lookups memoised by point | 4.26 GB | 5.81 GB | 216 s | None | 2.66 GB, 74 row groups |

- The first six runs wrote the same rows. Fingerprints of each trip's segments, each session, and
  each session's samples match.
- A flush leaves each day's compressed bytes in memory, about 0.7 GB per large day. `object_store`
  buffers the last partial chunk of a write as a slice of the row group's whole allocation, and
  uploads it on the file's next write or its close.
- With each day's file closed, the footprint returns to about 7.3 GB after each day. The peak is
  7.10 GB of legs and 2.81 GB for the day being written. The session phase's highest sample is
  7.01 GB, down from 8.60 GB.
- With legs in batches, the segment phase peaks at 2.25 GB. Its footprint falls from about 1.7 GB
  to 0.6 GB as the legs' strings are released. The whole run's peak moves to the write of
  `train_session_v2`. `train_segment_v4` keeps its 16 row groups and grows by 0.06%.
- With trips released and each shape copied, the footprint starts sampling at 3.70 GB and reaches
  4.41 GB 215 s later. Session paths and sample buffers grow over the same time. It falls to 2.89 GB
  as sampling ends. Closing the sample files then frees 0.65 GB, and the write of `train_session_v2`
  reaches about 2.6 GB.
- With legs passed by value, the footprint starts sampling at 4.96 GB, 1.26 GB higher.
  `polyline::decode_polyline` pushes each point onto a `Vec`, so the `Vec` ends with spare capacity.
  A test grew 1.5 million vectors to hold 3.11 GB of points. Its peak footprint was 4.32 GB as
  grown, 4.32 GB after `shrink_to_fit`, and 3.55 GB after a clone. The clone cost 0.19 s.
- `/usr/bin/sample` took two 10 s stack samples during the session phase of the run with each shape
  copied. They put 82% of the main thread's time in `position_at`, 2% in `containing`, 2% in
  `session_of`, and 6% in writing samples. The other threads sat idle.
- The segment phase of that run took 274 s, against 223 s for the same code in the run before. Its
  total time therefore shows no slowdown from the copy.
- `footprint` labels binary gigabytes GB, so its figures read 7% below those of `time -l`. That
  run's peak reads 4.43 in the first and 4.76 in the second.
- With each leg's distances computed once, the session phase takes 150 s, against 661 s. The peak
  stays early in sampling, since each trip holds its shapes until it is sampled.
- Those distances move 364,763 of the 64.2 million samples, by at most 1.1e-13 degrees, under 13 nm.
  Implied speeds change by at most 1e-9 m/s. Times, sources, trips, sessions' sample counts, and
  `train_segment_v4` all match. The comparison leaves out `geometry`, `geometry_projected`, and
  `bbox`, which derive from the positions.
- Stack samples of that run put `containing` at 61% of the segment phase and `geo_day` at 23%, most
  of it projection. In the session phase, they put `GeoRowWriter::write` at 30%, `session_of` at
  12%, `containing` at 12%, and `sampled` at 9%.
- With lookups memoised, the segment phase takes 86 s, against 222 s, and the session phase 130 s,
  against 150 s. Fingerprints match the run before exactly. The memo frees no memory, so the peak's
  0.42 GB fall has another cause, such as variation between runs.
- Stack samples of that run put `geo_day` at 58% of the segment phase, with projection at 35%, and
  writing the file at 24%, with zstd at 18%. In the session phase, they put `GeoRowWriter::write` at
  35%, `session_of` at 14%, and `sampled` at 10%. `containing` no longer shows in either phase.
- The runs started with 8.2 GB, 10.5 GB, 13.3 GB, 12.0 GB, 9.3 GB, 10.6 GB, 10.4 GB, and 13.1 GB of
  swap in use, so their resident sets and swap growth do not compare directly.

#### `motis_ingest` on the captures to 22 October

On 22 October, `motis_ingest` read 1,975,310 captured rows, which collapse to 1,319,928 legs.

- A full run took 666 s and wrote 44.9 million train session samples. `/usr/bin/time -l` reported
  a peak memory footprint of 12.7 GB and a peak resident set of 7.9 GB, on a machine with 24 GB.
  Swap grew by 5.4 GB.
- The segment derivation alone reaches that peak. An earlier run died writing `train_segment_v2`,
  before the session derivation began, with a peak memory footprint of 12.7 GB too.
- A run needs disk room for swap as well as for what it writes. That earlier run started with
  about 4 GB of disk free, and its swap growth filled the disk. The capture then lost its polls at
  19:10 UTC and from 19:30 to 21:50 UTC.

#### The baseline side-by-side pass, `delfi-2026-10-10.plain`

The pass of 13 October 2026 ran from 21:51 to 23:51 UTC, at night, so it holds few trains: 160
joined legs, 145 of them regional.

**Joining.** Every regional leg joined. Of 30 long-distance and high-speed legs, 15 joined, 9
came from the local server alone, and 4 from Transitous alone. Most of the unjoined legs belong
to `12N`, which DELFI carries as several trips with identical times. The two servers return
different ones, and split them at different stops. Transitous returns 4 more legs from feeds
other than DELFI, all night trains.

**Shapes.** Every local shape is a straight line between its stops. The distance to Transitous's
track shape measures that straight line, not disagreement between two track shapes: a median
of 519 m on regional legs, 8 km on high-speed legs, and 36 km on long-distance legs. This pass
cannot set a shape threshold. The pass after `route_shapes` can.

**Arrivals.** Where both servers had realtime, all 145 regional legs agree within 1 minute, 143
of them exactly. The local server had no realtime for any high-speed leg, and Transitous had it
for 5 of the 10, which differ by up to 71 minutes. One long-distance leg had realtime on both,
yet Transitous forecast a delay of 7–11 minutes that the local server never showed.

**Threshold.** Two arrivals agree when they differ by at most 1 minute.

#### Two passes compared: `delfi-2026-10-10.plain` and `delfi-2026-10-10.route-shapes`

The baseline ran at night, from 21:51 to 23:51 UTC on 13 October. The route-shapes pass ran at
midday, from 10:53 to 12:43 UTC on 17 October, and holds 1,139 joined legs against the baseline's
160.

**Shapes.** In the route-shapes pass, every joined leg lies within 12.6 m of Transitous's shape.
The baseline's medians were 519 m on regional legs, 8 km on high-speed legs, and 36 km on
long-distance legs. On the 119 segments both passes compared, the regional median falls from
520 m to 0 m, and the high-speed median from 8 km to 1.3 m. The two servers' shapes are identical
on 84% of regional legs and 65% of high-speed legs.

**Joining.** Every regional leg joined in both passes. The route-shapes pass leaves 10 high-speed
and long-distance legs unjoined, against 13 in the baseline. Five of its six local-only legs depart
from 12:22 UTC on, in the pass's last half hour. All four Transitous-only legs departed between
09:49 and 10:31 UTC, before the pass began. One of them, FLX10, carries stop ids with a `_G`
suffix the local server does not use.

**Arrivals.** Regional arrivals agree in both passes, with a 95th percentile of 0 minutes in each.
The widest regional difference is −6 to +4 minutes, in the route-shapes pass. Transitous had
realtime for all 40 high-speed legs in the route-shapes pass, and the local server for 9. Without
local realtime, high-speed arrivals differ by at most 13 minutes, against 71 in the baseline. With
it, they differ by a median of 7 minutes. The three largest differences, 19 to 38 minutes, are all
on ICE 599, whose delay Transitous carried and the local DELFI GTFS-RT feed did not.

**For a shape threshold.** The largest shape distance in the route-shapes pass is 12.6 m, on both
regional and high-speed legs.

#### The captures under `delfi-2026-10-10.route-shapes`

The label covers the pass and a Germany-wide local capture, with no region, from 23:03 to 23:53
UTC on 17 October. Transitous's capture that day ended at 12:53 UTC. The comparison joins only
polls both servers answered, so its findings leave out the Germany-wide capture's 3,823 rows.

#### The tolerance in `off_track_share`

`off_track_share` takes its 20 m tolerance from Bast and Brosi's evaluation of pfaedle. That
evaluation counts a stretch between stations as matched below a Fréchet distance of 20 m from the
true course.
