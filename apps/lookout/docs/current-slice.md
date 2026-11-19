# Current Slice: Motis as ground truth for the UK

### Target

Replicate for the UK what the slice "Motis as ground truth" builds for Germany. A local Motis
server draws rail along the track and gives realtime arrival times. A week-long capture of the
whole country comes from it.

### Straw Man

Take [2026-10-23-great-britain-capture.md](2026-10-23-great-britain-capture.md) as the straw man.

### Refactor

The German setup moves from the top level of `tools/motis-server` into a subdirectory, so the
UK's setup can sit beside it. The Justfile, `motis_server`, and `geo_data` at that top level serve
Germany alone.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-11-17.

#### The server

- **The server imports Aubin's timetable and polls Aubin's realtime feed**, as Transitous does. Both
  URLs answered without a key on 17 November. The timetable is a 1.49 GB zip, last modified on 16
  November, and the realtime feed is 9 MB. This takes config alone. A bridge from Darwin needs a
  Rail Data Marketplace account and new code. TravelWhiz's timetable has no realtime feed whose
  trip ids are known to match it.
- **Each country's server sits in its own directory under `tools/motis-server`, as a just module.**
  `de/` and `gb/` each hold a Justfile, `motis_server`, and `geo_data`. The top Justfile keeps
  `VERSION`, `TARGET`, and `prerequisites`, and declares `mod de` and `mod gb`. A recipe runs as
  `just de motis_server`. `apps/lookout/Justfile` reads `VERSION` from the top Justfile, and keeps
  doing so. One Justfile with a country argument on every recipe loses to this, since the two
  setups share few steps.
- **Motis moves to v2.11.4 first, in a commit of its own.** v2.11.4 was released on 21 October,
  and the rule in `.claude/rules/lookout-upgrades.md` upgrades what a slice touches before the slice
  starts. The client's API spec is fetched again for the new version.
- **The UK import skips tiles and geocoding.** Tiles and geocoding took 17.5 GiB of the German
  server's 26 GiB on 23 October. The capture uses neither.
- **The German server listens on port 10000 and the British one on 10001.** Each module sets
  `PORT`, and `server_port` writes it into the imported config. Another project on this machine
  holds 8081, and moving both servers keeps their ports together.
- **A script of our own, `gb-rail.lua`, sets Transitous's route types and drops every route that is
  not rail.** Transitous's `gb-great-britain.lua` is under the MIT licence, which permits copying
  it into the tree with its credit. Dropping buses at import keeps Aubin's bus timetable out of the
  shapes the import computes.
- **The download drops `shapes.txt` from Aubin's timetable**, as Transitous does. It takes 6.2 GB
  of the 13.3 GB unpacked, and `route_shapes` computes rail shapes afresh. A 2.3 GB `shapes.txt`
  coincided with broken realtime in Germany, as `docs/motis.md` records. Keeping the file is the
  alternative, and the straw man took it.

#### The capture

- **The poller warns when a poll's realtime share falls below a floor.** The realtime share is the
  share of a poll's captured legs that Motis marks realtime. `--min-realtime-share` sets the floor,
  and with no floor the poller only logs the share. A fixed floor beats a drop relative to the
  previous poll, since the share differs by mode and by country: German high-speed rail held
  10–16% even before its realtime fell.
- **The UK floor is 0.9.** Across the Scotland trial's polls the share held 0.975–0.994, and
  regional rail, 95% of legs, was marked in every one. A dead feed takes the share to about 0.05.
  A floor near 0.98 would warn whenever one more train went unmarked.
- **The UK capture runs through landing**, as Germany's does, with `--country GB`.

#### Consequences elsewhere

- `Captured` in `crates/motis` gains a count of the legs Motis marks realtime.
- `crates/motis/README.md` and `docs/motis.md` link to the top Justfile, and name the German
  recipes by their new paths.
- The German server sets up afresh in `de/`. Its old data, 13 GiB in the main checkout on 17
  November, goes.

#### Rejected / deferred

- **A bridge from Darwin**, unless Aubin's feeds prove unusable. It needs a Rail Data Marketplace
  account, a Kafka consumer, and a GTFS-RT server.
- **Asking Aubin before polling.** Transitous polls the realtime feed without a key, and the server
  fetches it once a minute. A reply from Aubin asking for less brings this back.

#### Open questions

- Whether the share falls below 0.9 in the small hours, when the unmarked sleeper portions make up
  more of the few trains running. The trial's share was already falling as it ended, and the first
  night of the week-long capture answers it. If the share falls, the poller gains a minimum count
  of legs below which it does not warn.

### Observations

- **The Scotland trial, 20:22–00:22 UTC on 18 November**, labelled
  `aubin-2026-11-18.route-shapes`. `notebooks/realtime_share/trial.py` reads it from landing.
  Its 25 polls held a share of 0.975–0.994, with 2–4 trips unmarked in each. From 23:32 the
  trips fell from 115 to 42 a poll, while 2–3 stayed unmarked, and the share fell from 0.989 to
  0.975.
  - Regional rail: all 6,708 legs marked.
  - Long-distance: 0.916 of 273 legs. Three trips were unmarked, each from its first poll in
    Scotland to its last: two CrossCountry trips ending in Edinburgh, and one Avanti West Coast
    trip ending in Glasgow. They had started at 11:15, 14:27, and 19:30, hours before their first
    leg in Scotland.
  - Night rail: 0.381 of 113 legs. Two Caledonian Sleeper trips, one on the West Highland line and
    one reaching Aberdeen, were unmarked in every poll. Their trip IDs name the main train,
    `C04565`, which was marked in all 23 of its polls. The two trips starting at 23:41 were
    marked in all 5 of theirs.

### Tasks

#### Refactors / extensions

- [x] Move Motis to v2.11.4 in `tools/motis-server/Justfile`, and run `just motis-openapi`. Rebuild
      `crates/motis`, and run its tests.
- [x] Move the German recipes into `tools/motis-server/de/Justfile`, as a just module. The top
      Justfile keeps `VERSION`, `TARGET`, and `prerequisites`. A module does not see its parent's
      variables, so `motis.just` holds the three, and the top Justfile and `de/Justfile` import it.
- [-] Move the German server's `motis_server` and `geo_data` from the main checkout into `de/`,
      and start the server from there. Moot: the German server sets up afresh in `de/` instead.
- [ ] Set up the German server afresh in `de/`, under v2.11.4, with `just de setup`. The user
      deletes the old data in the main checkout.
- [x] Update the links in `crates/motis/README.md` and `docs/motis.md`.

#### The realtime warning

- [x] Count realtime legs in `Captured`, and log the realtime share in "polled motis".
- [x] Make the poller log a warning when a poll's share of realtime legs falls below the floor
      `--min-realtime-share` sets in `PollArgs`. The German capture
      lost its realtime on 20 October 2026, and nothing reported it, as
      [its caveat](completed-slices.md#caveat-realtime-stops-partway-through-the-capture) sets out.

#### The UK server

- [x] Add `tools/motis-server/gb/Justfile`, downloading Geofabrik's `great-britain-latest.osm.pbf`
      and Aubin's timetable into `gb/geo_data`.
- [x] Read the dates the timetable's calendar covers, and record its horizon in `docs/motis.md`.
- [x] Add `gb/motis_server/scripts/gb-rail.lua`, with Transitous's route types, its credit, and a
      `process_route` that drops every route that is not rail. It sits in `gb/scripts`, since git
      ignores `motis_server`.
- [x] Add a `motis_setup` recipe: config, `route_shapes` as Germany has it, the script, no tiles,
      no geocoding, and its own port. Add `setup` beside it, as `de/Justfile` has.
- [x] Import, and record the time, peak memory, data size, routes matched, and segments beelined.
      The sandbox refuses the `route_shapes` step, which the user ran outside it.
- [x] Add an `enable_rt` recipe, putting Aubin's realtime feed in the dataset's `rt` list with a
      `User-Agent` naming lookout.
- [x] Start the server, and read realtime resolution from its log.
- [x] Move the German server to port 10000 and the British one to 10001, with `server_port`, and
      the client's default to 10000.

#### The Scotland trial

- [x] Capture `--region GB-SCT` for a few hours, with a label naming the timetable build.
- [x] Read the realtime share by mode, and the route types ScotRail and the unscripted operators
      take. Choose the floor for the week-long capture. ScotRail's routes take regional rail. The
      trial held no unscripted operator.
- [ ] Drain the trial's landing into bronze with `just bronze-promote-motis-landing`, once its four
      hours end and before the side-by-side starts. Rerun `notebooks/realtime_share/trial.py` on
      all four hours first, since it reads landing only. Commit the bronze it writes.
- [ ] Capture Scotland from the local server and Transitous together, and compare shapes and
      arrivals, as the German side-by-side pass did.

#### The capture

- [ ] Capture all of Great Britain for a week through landing, at the chosen floor. Only after the
      Scotland trial.
- [ ] If the warning fires during a capture, investigate the cause before the capture ends. Compare
      the realtime feed's trip ids with the imported timetable's.
- [ ] Drain landing into bronze, and derive the train segments and train sessions.
- [ ] Report the realtime share by day and mode, as the German caveat's table does.

#### Wrap-up

- [ ] Fold what holds of `docs/2026-10-23-great-britain-capture.md` into `docs/motis.md`, and
      delete it.
- [ ] Fold what holds of `docs/2026-11-15-maplibre-tile.md` into `docs/web.md` and
      `docs/medallion.md`, as its header directs, and delete it.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.
