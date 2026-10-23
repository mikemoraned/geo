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
[2026-10-23-compact-bronze.md](2026-10-23-compact-bronze.md).

- **Polls land in landing, and a drain moves them into bronze.** Bronze never rewrites a file, and
  git keeps every committed blob, so polls merge before they reach git. Merging poll files after
  committing them frees nothing. Landing sits at `data/medallion/landing/`, ignored by git.
  `docs/medallion.md` already describes landing as the capture log a polling process appends to,
  drained into bronze. Confirmed 2026-10-23.
- **A drain writes the polylines bronze lacks, then the polls.** It appends the polylines
  `motis_polyline` lacks as one new file, each keyed by a UUID v5 of its text, as a source's id is.
  It then writes the polls to `motis_segment_v3`, one zstd file per `polled_date`, each row carrying
  a polyline id in place of its polyline. Rows sort by stop pair, polyline id, trip, and capture
  time. Draining every 10–24 hours, a day then takes 5.4–7 MB in git, against 58–70 MB as poll
  files. Appending every polyline a drain sees, rather than the missing ones, adds at least 7 MB a
  day. Confirmed 2026-10-23.
- **A drain is safe to re-run.** It writes polylines before polls, reads both back, and only then
  deletes the landing files. A re-run skips any poll whose source and capture instant bronze
  already holds. A crash then leaves at worst an unused polyline, never a lost or doubled poll.
  Confirmed 2026-10-23.
- **`writer_at` compresses with zstd, for bronze and silver alike.** It writes both layers, so
  compressing both needs no writer properties per layer, as compressing bronze alone would. It also
  shrinks silver on disk, which an ingest run needs room for. Confirmed 2026-10-23.
- **The running capture finishes as it started.** The poller keeps writing `motis_segment_v2` until
  about 25 October, committed and pushed as now, as a backup the rewrite later drops. Stopping it
  mid-capture risks a gap. Confirmed 2026-10-23.
- **A one-off binary migrates `motis_segment_v2` into `motis_segment_v3` and `motis_polyline`.** It
  is committed with the data it writes, then removed, as earlier migrations were. The drain then
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
- [ ] Make every checked newtype check its value whenever it is deserialised, as `Label` does:
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

#### Six-day capture

Before the branch merges, so `main` never holds a poll file. Before Optimisation, since the
polyline table changes what `motis_ingest` reads.

- [ ] Finish the six-day capture of Germany, which started on 19 October and ends around 25
      October. Commit its polls as they accumulate, and its last ones once the poller stops. Then
      run `just silver-motis-ingest`, so the train sessions cover the whole week.
- [ ] Give `writer_at`, in `crates/medallion/src/write.rs`, zstd compression, for bronze and
      silver alike.
- [ ] Add bronze `motis_polyline` and `motis_segment_v3` to `crates/medallion-model`, with a
      `PolylineId` newtype holding the UUID v5 of a polyline's text.
- [ ] Add a landing dataset for the capture's polls, on `layers::Landing`, which no dataset uses
      yet. Give `medallion` a way to list its files and delete one once drained. Ignore
      `/landing/` in `data/medallion/.gitignore`.
- [ ] Point `SegmentLog`, in `crates/motis/src/bronze.rs`, at the landing dataset, and rename the
      `bronze-poll-*` recipes `landing-poll-*`.
- [ ] Add `motis_drain` to `crates/motis/src/bin/`, and a recipe `bronze-drain-motis`, draining
      landing into `motis_polyline` and `motis_segment_v3`.
- [ ] Read `motis_segment_v3` in `captures`, in `crates/motis/src/ingest.rs`, joining
      `motis_polyline` after picking each leg's newest capture.
- [ ] Migrate `motis_segment_v2` into `motis_segment_v3` and `motis_polyline` with a one-off binary,
      writing through the drain's code. Commit it with the data it writes, and remove it in a later
      commit. Only after the capture ends.
- [ ] Record, under Observations, the sizes of the migrated files against the research doc's
      DuckDB figures.
- [ ] Check `just silver-motis-ingest` derives the same `train_segment_v2` and `train_session`
      rows from `motis_segment_v3` alone as from `motis_segment_v2` alone.
- [ ] Point the other readers of `motis_segment_v2` at `motis_segment_v3`, joining
      `motis_polyline`. These are the `motis_capture` notebooks
      `compare_delfi-2026-10-10.plain_route-shapes.py`,
      `germany_delfi-2026-10-10.route-shapes.within.py`, both `side_by_side_delfi-2026-10-10`
      notebooks, and `v1.py`, and the shared test setup in `crates/motis/tests/common/mod.rs`.
- [ ] Delete `motis_segment_v2`'s files, `MOTIS_SEGMENT_V2`, its entry in
      `crates/medallion-model/src/bronze.rs`, and its reading in `captures`. Its rows live on in
      `motis_segment_v3`.
- [ ] Describe landing, the drain, `motis_polyline`, and `motis_segment_v3` in `docs/medallion.md`
      and `docs/motis.md`, in place of `motis_segment_v2`. A third-party service's samples take one
      bronze file per drain. Then fold what holds of `2026-10-23-compact-bronze.md` into them, and
      delete it.
- [ ] Rewrite this branch's commits since it left `main` without the `motis_segment_v2` files,
      using `git filter-repo` in a fresh clone. Before opening the PR, force-push the result.

#### Optimisation

After Refactoring, once the slice works end to end.

- [ ] Cut the memory `motis_ingest` takes. On the capture to 19 October, it peaked at 7.6 GB
      and wrote 15 million train session samples in 302 s. Profile it by phase before changing
      it. Three costs are suspected. Every collapsed leg is held twice, once for segments and
      once for sessions. Each session's path runs through every sample, held in degrees and
      projected. Each open partition file buffers a row group of up to a million rows. A path
      from the legs' shapes would also shrink `train_session`, at 460 MB on disk.

#### Wrap-up

- [ ] Add the Motis source to the derivation graph in `docs/architecture.md`.
- [ ] Fold what holds of `2026-10-12-track-shapes-and-realtime.md` into `docs/motis.md`: the
      `route_shapes` setup, the train-name script, realtime resolution, and the measured import.
      Correct the `displayName` claim there. Then delete the research doc.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.

### Investigation

#### A pass split by imprecise samples

Re-deriving `session_crossing` with a pass per visit, on 2026-10-18, gave two sessions passing one
crossing twice. Session `d4d6e062` reverses at Frankfurt (Main) Hbf, a terminus, and crosses the
Main out and back, 13 minutes apart. Its samples report 2–5 m of accuracy, and it is the first geo
test case.

Session `43fffff4`, on 26 July near (50.355, 11.023), passes four crossings twice each, 22 s apart.
Its samples report about 1,450 m of accuracy. Their distance from one crossing runs 6,682 m, 234 m,
588 m, 155 m, and 2,873 m. The middle sample falls outside the 250 m radius, so one approach splits
into two visits. Samples that imprecise cannot show whether the train passed the crossing at all.

The visit rule cannot tell such jitter from a real revisit. A real revisit needs the train to leave
and return, which takes a span of space and time. Where a session's uncertainty exceeds that span,
two visits are no longer discernible. A representation of this in space and time, akin to scale
space in images, is a research task in the slice "Evaluation framework with associated Crow Flies
predictor improvement".

### Observations

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
