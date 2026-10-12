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
  both. Changed on 2026-10-07 from one silver dataset with an optional source id.
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
  legs touching the area.
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

#### Consequences elsewhere

- `docs/device.md` and the pin comment in `crates/platform/web/core/Cargo.toml` say crux after
  0.16.2 reboots the device.
- The workspace `Cargo.toml` comment ties geo to sedona-geo's version. The SedonaDB 0.5.0 slice
  moves geo from 0.32 rather than 0.31.
- `App` in `crates/platform-core/src/app.rs` loses `type Capabilities` and the `_caps` argument.
- `silver/session_crossing` can hold several rows per (session, crossing), so the packed sessions
  and `sessions.version` change.
- The derivation graph in `docs/architecture.md` changes.

#### Open questions

- How a source records whether its server computes rail shapes. A server registers one source id
  whether `route_shapes` is on or off. The id hashes only the base URL, the version, the feed, and
  the area.
- Which feed a local source names. `local` covers both the July captures, on gtfs.de, and today's
  DELFI server.

#### Rejected / deferred

- **pfaedle, for now.** Its rail shapes followed the track, but importing a feed carrying them
  broke realtime, for a reason never found. Motis's `route_shapes` import step computes shapes and
  leaves the feed alone. If Motis issue #1596 stays open and wrong parallel lines prove common,
  pfaedle's line-relation penalty is the reason to return.
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
- [ ] Log, for each poll, how many requests of each kind it sent to Motis and how long each kind
      took in total. A capture of Thuringia on 2026-10-11 took 5.0, 4.7, and 8.9 minutes per poll,
      though the second and third looked up only 8 and 16 new trips, so the trip lookups alone do
      not explain the time.
- [ ] Record in a source's area the Overture release its shape came from. A new release can move
      a region's outline, yet a capture at it now registers under the same source id.
      The id covers the release from here on. Only this branch has written `motis_source`, so the
      change is in place, with no new dataset version:
      1. Add the release to `Area` and `MotisSourceRow`, and to the fields the id hashes.
      2. Backfill the one existing source with duckdb, not Rust: release `2026-07-22.0`, from DE
         extract `20260804T152143Z`, the newest when it was captured on 2026-10-11. Rewrite its
         `source_id` in `motis_source` and in the three `motis_segment_v2` files to the new id.
      3. Read the backfilled source and segments back through Rust.
      4. After `just bronze-init` and `just silver-init` finish at `2026-09-23.1`, run a short
         capture of Thuringia, and check it registers a second source.
- [ ] Describe the source and `motis_segment_v2` in `docs/medallion.md`, and the area mode,
      Transitous, and its usage policy in `docs/motis.md`.

#### One train_segment version

Next after Motis capture. Bronze stays as written: `motis_segment` keeps its rows, and ingest
maps them into `train_segment_v2`.

- [ ] Record a source for the `motis_segment` captures: the local server, feed `local`, Motis
      v2.10.2, the version `tools/motis-server/Justfile` pinned from 19 July. Derive their legs
      into `train_segment_v2` under it.
- [ ] Remove `train_segment` and `TrainSegmentRowV1` once nothing reads them.

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
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.
