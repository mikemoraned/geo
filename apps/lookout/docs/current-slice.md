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

- **A pass per visit, at an interpolated instant.** `match_crossings` keeps one pass per
  (session, crossing) today, at the nearest fix, which at line speed lies over 100 m off. It
  instead splits the fixes within the match radius into visits, and interpolates each pass's
  instant between the bracketing fixes.

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
- [ ] Capture DE for a day. The user runs this.

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
- [ ] Measure `off_track_share` over the day's DE capture: the share of a leg's length lying more
      than 20 m from every rail segment in the country's Overture extract, sampled every 25 m.
      `notebooks/motis_capture/germany_delfi-2026-10-10.route-shapes.within.py` derives it. If any
      leg within Germany scores above 0, add it to `train_segment_v2`, with the extract's id beside
      it.
      Otherwise the poller's own filter, which keeps only legs within the country, is enough.
- [x] Remove `train_segment` and `TrainSegmentRowV1` once nothing reads them.

#### Ground truth

- [ ] Record a pass per visit in `crates/session_crossings/src/matching.rs`. Test it on a session
      that reverses over a crossing.
- [ ] Interpolate each pass's instant between the bracketing fixes.
- [ ] Run `just silver-session-crossings` and `just gold-pack-sessions`, and move
      `sessions.version`.

#### Refactoring

At the end of the slice, once its features are in.

- [ ] Look across the lookout crates for types a library or `std` already provides, as
      `TimeWindow` was a `Range<DateTime<Utc>>`, and for raw numbers standing in for a type.
      Replace each where the code reads better.
- [ ] Name every flag and field whose value is Overture's own after Overture, as
      `--overture-country` and `--overture-region` are. Check the other crates follow it.
- [ ] Apply the `Age` rule in `.claude/rules/rust.md` across `lookout`: every maximum age,
      lookback, or other age held as `domain::Age`, every age flag read as a `Span`, and
      `Option<Age>` where no age applies.
- [ ] Take every duration, interval, and age a person writes across `lookout` as a human-readable
      `Span`, such as `30s` or `4h`, as the Motis pollers do. Move `Span` out of `crates/motis` to
      where every crate can reach it.

#### Wrap-up

- [ ] Add the Motis source to the derivation graph in `docs/architecture.md`.
- [ ] Fold what holds of `2026-10-12-track-shapes-and-realtime.md` into `docs/motis.md`: the
      `route_shapes` setup, the train-name script, realtime resolution, and the measured import.
      Correct the `displayName` claim there. Then delete the research doc.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.
