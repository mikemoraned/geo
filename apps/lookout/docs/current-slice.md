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
- **The UK import skips tiles and geocoding, and listens on port 8081.** Tiles and geocoding took
  17.5 GiB of the German server's 26 GiB on 23 October. The capture uses neither. Port 8081 lets
  both servers run together.
- **A script of our own, `gb-rail.lua`, sets Transitous's route types and drops every route that is
  not rail.** Transitous's `gb-great-britain.lua` is under the MIT licence, which permits copying
  it into the tree with its credit. Dropping buses at import keeps Aubin's bus timetable out of the
  shapes the import computes.

#### The capture

- **The poller warns when a poll's realtime share falls below a floor.** The realtime share is the
  share of a poll's captured legs that Motis marks realtime. `--min-realtime-share` sets the floor,
  and with no floor the poller only logs the share. A fixed floor beats a drop relative to the
  previous poll, since the share differs by mode and by country: German high-speed rail held
  10–16% even before its realtime fell.
- **The UK capture runs through landing**, as Germany's does, with `--country GB`.

#### Consequences elsewhere

- `Captured` in `crates/motis` gains a count of the legs Motis marks realtime.
- `crates/motis/README.md` and `docs/motis.md` link to the top Justfile, and name the German
  recipes by their new paths.
- The German server's data, 13 GiB in the main checkout on 17 November, moves into `de/` rather
  than downloading and importing again.

#### Rejected / deferred

- **A bridge from Darwin**, unless Aubin's feeds prove unusable. It needs a Rail Data Marketplace
  account, a Kafka consumer, and a GTFS-RT server.
- **Asking Aubin before polling.** Transitous polls the realtime feed without a key, and the server
  fetches it once a minute. A reply from Aubin asking for less brings this back.

#### Open questions

- The floor for each capture. The German capture held 85–90% across all rail before its fall,
  under v2.11.3. v2.11.4 marks fewer legs realtime, as `docs/motis.md` sets out, so the UK floor
  waits on the Scotland trial.
- How far ahead Aubin's timetable runs, and how often Aubin rebuilds it. If it runs only days ahead,
  the week-long capture needs fresh imports, each a new run with its own label.
- Whether Aubin's trip ids resolve against its own realtime feed as DELFI's do, at about 99.9%.

### Tasks

#### Refactors / extensions

- [x] Move Motis to v2.11.4 in `tools/motis-server/Justfile`, and run `just motis-openapi`. Rebuild
      `crates/motis`, and run its tests.
- [ ] Move the German recipes into `tools/motis-server/de/Justfile`, as a just module. The top
      Justfile keeps `VERSION`, `TARGET`, and `prerequisites`.
- [ ] Move the German server's `motis_server` and `geo_data` from the main checkout into `de/`,
      and start the server from there.
- [ ] Update the links in `crates/motis/README.md` and `docs/motis.md`.

#### The realtime warning

- [ ] Count realtime legs in `Captured`, and log the realtime share in "polled motis".
- [ ] Make the poller log a warning when a poll's share of realtime legs falls below the floor
      `--min-realtime-share` sets in `PollArgs`. The German capture
      lost its realtime on 20 October 2026, and nothing reported it, as
      [its caveat](completed-slices.md#caveat-realtime-stops-partway-through-the-capture) sets out.

#### The UK server

- [ ] Add `tools/motis-server/gb/Justfile`, downloading Geofabrik's `great-britain-latest.osm.pbf`
      and Aubin's timetable into `gb/geo_data`.
- [ ] Read the dates the timetable's calendar covers, and record its horizon in `docs/motis.md`.
- [ ] Add `gb/motis_server/scripts/gb-rail.lua`, with Transitous's route types, its credit, and a
      `process_route` that drops every route that is not rail.
- [ ] Add a `motis_setup` recipe: config, `route_shapes` as Germany has it, the script, no tiles,
      no geocoding, and port 8081.
- [ ] Import, and record the time, peak memory, data size, routes matched, and segments beelined.
- [ ] Add an `enable_rt` recipe, putting Aubin's realtime feed in the dataset's `rt` list with a
      `User-Agent` naming lookout.
- [ ] Start the server, and read realtime resolution from its log.

#### The Scotland trial

- [ ] Capture `--region GB-SCT` for a few hours, with a label naming the timetable build.
- [ ] Read the realtime share by mode, and the route types ScotRail and the unscripted operators
      take. Choose the floor for the week-long capture.
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
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.
