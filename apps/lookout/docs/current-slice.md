# Current Slice: extend to UK

### Target

We've mostly been testing with German (DE) data so far. We should extend what we have with Germany data to the UK train network. 

This means, for example:
* having an Overture dataset snapshot for the UK 
* where needed, use a single UTM Zone that is acceptable for UK (note will need to add to PROJJSON extract, for example)
* injest the sessions that I've recorded in Bronze for the UK into Silver, as we should now be able to represent them
* derive a water crossings dataset for the UK
* update the Kiosk dataset to include a mix of session data across all countries, where possible i.e. find the top N entries in each country.
* end up fully up to date on the latest Overture release: mirror it locally, bump the pinned release, re-extract DE and GB at it, and re-derive silver and gold from those.

It's ok if we don't have motis data for the UK i.e. it's ok if we don't have live train data for the UK as part of this slice.

Some invariants:
* The derived point dataset of water crossings (which should now include DE+UK data) must still be small enough to be downloaded and fit in the m5plus
  * if this isn't possible then we need to use a more compact representation. however, let's avoid doing that if we don't need to

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-09-27.

- **The UK is `GB`.** That is its ISO 3166-1 alpha-2 code, which is what Overture's `country`
  column carries and what the `country=` partition already states. `UK` would match no
  upstream row.
- **The UK's projected zone is EPSG:25830, ETRS89 / UTM zone 30N.** Every engine has to agree
  on the projected column, and 25830 needs no datum transformation: `+proj=utm +zone=30
  +ellps=GRS80`, the same shape as Germany's 25832. Nothing sits between the fix and the grid
  for one engine to apply and another to skip, and nothing has to be kept in step as either
  moves. GB spans zones 29 to 31, so the scale factor reaches about 1 m per km at the Western
  Isles and in East Anglia — comparisons are made within one country, so a zone that is not
  comparable with Germany's costs nothing. Checked against PROJ: 443,797.38, 6,200,880.49 for a
  point near Edinburgh, to the centimetre. It beats British National Grid; see [Rejected /
  deferred](#rejected--deferred).
- **proj4rs goes to 0.2 in this slice, whatever zone the UK gets.** 0.1 resolves
  `crs-definitions` 0.4.0, whose definitions name no datum, so any CRS on a local datum
  projects about 100 m out and reports nothing. Nothing the store projects today sits on
  one — CRS84, 25832 and 25830 are all geocentric — which is the argument for doing it now
  rather than when a country whose zone is on a local datum arrives and the error lands in
  silver unannounced. Checked as a plain version bump: `from_epsg_code` and `transform` are
  unchanged, and Berlin through 25832 still gives 798,809.63, 5,828,000.60, the numbers the
  existing test pins.
- **A country is added on its own, and adding one leaves the others standing.** Extracting a
  country writes a new extract beside the ones already there, and a manifest row naming that
  country and the release it was read from; nothing already in the store changes. An extract
  stays one country and one release, which is what bronze's immutability rests on: an addition
  never reopens an id already recorded. What places a point is therefore the union
  across countries — each country's own newest extract — and not the store's newest extract,
  which is what `CountryAreas::newest` reads today. One `ORDER BY extracted_at DESC LIMIT 1`
  supplies the areas for every country in `ALL`, so taking the GB extract would leave every
  German session unplaceable. Three things follow, and the refactor below puts them in place
  before any UK data is taken:
  - **Countries need not share a release.** A country extracted from a year-old mirror
    snapshot sits beside one extracted from a current release, and each row records which
    release it came from. Sharing a release, as DE and GB do here, buys comparability between
    them and is a choice per slice rather than a rule.
  - **A country can be added long after the rest.** The mirror holds every release, so the
    only thing an addition needs is that release still being readable somewhere.
  - **Extracting a country twice shadows rather than doubles.** The union takes each
    country's newest extract, so a re-extraction supersedes the one before it and no row is
    counted twice.
- **GB is extracted at release 2026-07-22.0, from the local Overture mirror.** Both countries
  then sit on the release the DE crossings were derived from, and a crossings figure compares
  across them. The recorded releases have aged out of the public bucket, so the mirror is the
  only location that answers. The alternative, a current release for GB, would also mean
  bumping `DEFAULT_RELEASE` mid-slice.
- **The crossings notebook covers both countries in one run and one write.** `write_silver`
  replaces the whole dataset and deletes the country partitions the table does not cover, and
  it checks `crossing_id` and `crossing_compact_id` across the table it is given. One table
  keeps both properties honest with no change to the store.
- **That two-country notebook is a new version, `v10.py`.** The versions are the record of how
  the derivation changed, and `just silver-water-crossings` names the one in use.
- **The latest release is taken last, not first.** The UK work is done on 2026-07-22.0, where
  the DE crossings already sit and the mirror already answers, and the bump then moves both
  countries together and re-derives everything once. Bumping first would have the UK work and
  the release move fail together.
- **The mirror takes a whole release, not the themes an extract reads.** It serves work
  outside this repo as well, and a theme nobody anticipated is the kind of thing a mirror
  exists to make discoverable. Syncing `base/water`, `transportation` and `divisions` alone
  would be a fraction of the hundreds of GB, and would have to be re-run from the bucket each
  time a question reached past them — by which point the release may have aged out of it.
- **The mirroring recipes live at `tools/overture-mirror/`.** The mirror serves anything in
  the repo that reads Overture, not lookout alone, so it sits beside `tools/motis-server` and
  follows it: a Justfile carrying the fetch, with the reasoning in its comments. The mirror
  itself stays outside the repo, on an external drive — what the repo records is how it is
  made, not what it holds. The alternative, a recipe in `apps/lookout/Justfile`, ties a shared
  prerequisite to one app.
- **The kiosk keeps the best three sessions per country.** `--max-sessions` becomes a
  per-country count. `session_crossing` holds no geometry and so has no country level; the
  country comes from joining `session`, which is partitioned by it.
- **A second country does not threaten the compact id.** It is four bytes, and around 12,000
  crossings across both countries give roughly a 1 in 60,000 chance of two landing on one
  name. The write refuses a collision, so it surfaces as a failed write rather than a silent
  clash downstream.
- **A country is named to Overture by its GERS id, not by its code.** Confirmed 2026-09-27,
  mid-slice. A country appears in the divisions theme as two `division_area` rows, its land and
  its territorial waters, sharing the `division_id` of the country division — DE
  `567d1698-7209-4b94-b7b8-0bc71bde0104`, GB `ce3429b1-d5c6-4763-91e7-0107e26d613e`. Reading by
  that id names the entity; reading by `subtype = 'country' AND country = '<code>'` names
  whatever rows carry a label, which is the same two rows today and rests on the label holding
  across releases. GERS exists to make the first kind of reference survive a release, with a
  registry recording first seen, last seen and last changed, and a changelog per release. The
  codes stay where they are earned: the `country=` partition, the manifest row, and the ISO code
  `Country::code` answers. The cost is a literal per country, since a new country's id has to be
  read from a release before it can be added — the compiler asks for it, as the mapping is
  exhaustive over `Country`.
- **A GERS id is a newtype over a UUID, in `medallion-model`.** Every id a release carries is a
  36-character UUID, so that is what the type validates. It sits beside the Overture schemas
  rather than in `medallion`, whose `Country` carries standards alone — an ISO code, an EPSG
  zone — and rather than in `domain`, which builds for Xtensa and wasm and where nothing names
  an upstream entity. The mapping is therefore a function of `Country` rather than a method on
  it.
- **The window keeps the territorial waters.** Confirmed 2026-09-27. Both areas of the division
  go into the bounding box, as they did when the code selected them by label. For GB that reaches
  -14.02 rather than the land's -8.65, about 5.4 degrees of longitude of Atlantic, because the
  waters around Rockall are UK territorial waters while the rock is not UK land. That extra width
  holds water rows in the open Atlantic that no railway comes near, and a larger extract with
  them. Both areas stay: the extract exists to find water a railway meets, and a coastal crossing
  sits in the waters, so a window round the land alone would drop the rows the derivation looks
  for. A test in `countries.rs` holds it — a division with both areas places a point over the
  waters — and it fails if the read is narrowed to `class = 'land'`.
- **Partitioning geo silver by country needs no work.** ~~The country level is applied above a
  dataset's own partition key by the shared silver write path, not declared per dataset, so a
  second country lands in its own partitions and its own CRS as soon as `Country` knows it.
  `match_crossings` already sweeps `Country::ALL`, and so follows too.~~ **Refuted 2026-09-27.**
  The write needs no work; the *read* does. A second country gives the projected column two CRSs
  across the dataset's files, and SedonaDB refuses to plan a scan spanning them, so every read of
  such a dataset had to be scoped to a country. See the group below and
  [Observations](#observations).

#### Consequences elsewhere

- **Filling in bronze takes every extract recorded, not the newest one.** `just bronze-init`
  backfills the newest today, so a fresh worktree would fill in GB and leave DE missing. A
  bare `extract` therefore comes to mean every extract the manifest records, skipping the ones
  whose rows are already there and reporting what it filled in; `backfill <id>` still takes
  one. That beats an `--all` flag: what a checkout needs is all of them, so it is the default
  rather than something to remember. The first group below builds it.
- **Where the mirror is gets stated once, in `tools/overture-mirror/mirror.just`**, imported
  by that dir's Justfile and by `apps/lookout/Justfile` in place of its `overture_mirror`
  literal. A variable-only file adds no recipes to whatever imports it, and an importing
  Justfile's recipes still run from its own directory, so `just bronze-init` is unchanged —
  checked against just 1.46. The path moves with the drive, and a drive that moved would
  otherwise leave one of the two Justfiles pointing at nothing.
- Adding a country leaves bronze alone but re-derives silver whole. The crossings notebook
  writes every country in one go, so a third country means running all three — idempotent,
  and the cost of not building the country-scoped write below.
- `just crs-definitions` gains a second `projinfo` line, and a second PROJJSON file is
  committed beside it.
- The crossings a browser fetches roughly double from 184 KB, and the device's rodata from
  69 KB.

#### Rejected / deferred

- **EPSG:27700, British National Grid.** The better projection for GB — about 0.4 m per km of
  scale error at worst, against 25830's 1 m per km — and still not worth taking, because it
  needs a datum transformation and each engine picks that transformation for itself. proj4rs
  applies the Helmert; PROJ applies OSTN15 where the grid data is installed, which is more
  accurate and therefore different. Whether the two agree then depends on what is installed on
  a given machine: with no OSTN15 grid they match to the centimetre, and `brew install
  proj-data` moves the notebook while leaving the Rust where it was. That is a property of a
  machine rather than of the store, and needing no transformation is how 25830 avoids it.
  Revisit if a derivation needs sub-metre truth across the whole of GB, and pin one
  transformation for both engines when it does.

  The definitions have to carry the datum for any of that to hold. `crs-definitions` 0.4.0
  defines 27700 as `+proj=tmerc +ellps=airy`, naming no datum, so the transformation is
  skipped and a point lands 81 m out in Glasgow, 89 m in Edinburgh and 125 m in London. This
  is the format rather than the library: PROJ's own `projinfo -o PROJ EPSG:27700` emits the
  same bare string, a proj4 string being a lossy export of a modern CRS. proj4rs itself carries
  OSGB36 with its seven parameters and honours `+datum=`, `+towgs84=` and `+nadgrids=`, and
  `crs-definitions` 0.5.0 names the datum — on proj4rs 0.2 the grid answers 281,451.24,
  674,626.69 against pyproj's 281,451.24135, 674,626.68754. A zone on a local datum is
  therefore only as right as the definitions are current, which is one reason the store moves
  to proj4rs 0.2.

  A CRS is a projection plus a datum, and it is the datum half that differs here. Ordnance
  Survey's [A Guide to Coordinate Systems in Great
  Britain](https://www.ordnancesurvey.co.uk/documents/resources/guide-coordinate-systems-great-britain.pdf)
  puts it as (§3.2):

  > The term geodetic datum is usually taken to mean the ellipsoidal type of datum just
  > described: a set of 3-D Cartesian axes plus an ellipsoid […] The datum definition consists
  > of eight parameters: the 3-D location of the origin (three parameters), the 3-D
  > orientation of the axes (three parameters), the size of the ellipsoid (one parameter) and
  > the shape of the ellipsoid (one parameter).

  The grid's datum is OSGB36, on the Airy 1830 ellipsoid, and of those eight parameters the
  `+ellps=airy` in the definition supplies two. OS on the other six (§5.2.1):

  > it is not geocentric as GRS80 is: it is designed to lie close to the Geoid beneath the
  > British Isles […] So, the Airy ellipsoid differs from GRS80 in size, shape, position and
  > orientation, and this is generally true of any pair of geodetic ellipsoids.

  A GPS fix is in WGS84, whose origin is "the Geocentre (the centre of mass of the Earth)"
  (§4.1), so reaching the grid takes a datum transformation *and then* the projection — PROJ's
  [Geodetic transformation](https://proj.org/en/stable/usage/transformation.html) counts "5
  steps" for the equivalent journey, a Helmert transformation among them. Skipping it is what
  the offsets above measure. Nor is the offset a constant to subtract: it varies with position,
  by about 8 m over the Glasgow to Edinburgh corridor alone, and even done properly it carries
  error of its own. OS report (§6.2):

  > For the transformation from ETRS89 to OSGB36 in Britain, using a single Helmert
  > transformation will give errors of up to 3m (95%) in plan

  ETRS89 raises none of this, which is why 25830 wins here and why Germany's 25832 has never
  shown the problem: it is geocentric like WGS84, close enough that
  [OS](https://docs.os.uk/more-than-maps/geographic-data-visualisation/guide-to-cartography/coordinate-reference-systems)
  say "the difference between ETRS89 and WGS84 can be ignored for most purposes", so nothing
  sits between the fix and the projection for one engine to apply and another to skip.
- **A country-scoped silver write**, sweeping within one country's partition and checking ids
  against the rest of the dataset. Revisit when deriving one country's crossings costs enough
  that rebuilding both is the thing to avoid.

#### Open questions

- How many crossings GB yields, and so the packed size and the scan cost. The tasks measure
  both, and the Target's fallback to a more compact representation waits on those numbers.
- ~~Whether the recorded UK sessions pass five crossings and so reach the kiosk.~~ Answered
  2026-09-27: one of the 8 does, with 6 passes, which clears `--min-crossings 5` with one to
  spare. The other 7 matched nothing. So GB contributes a single session to the kiosk however
  `--max-sessions` is counted, and the per-country change below decides nothing for GB until a
  second UK session qualifies.
- ~~Whether Overture's GB country area includes Northern Ireland, and so how far west the
  window reaches.~~ Answered 2026-09-27: it does, and the window reaches -14.02 through the
  maritime area. See [Observations](#observations).

### Tasks

#### Refactors / extensions: adding a country leaves the others standing

The invariant above, in place before the GB extract is taken, and testable on the DE extract
alone.

- [x] Take the country areas from the newest extract of each country in
      `crates/transport/src/countries.rs`, registering each extract's `division_area`. A
      country with no extract contributes no areas rather than failing, since `ALL` names a
      country before its extract is taken. `CountryAreas::newest` becomes
      `newest_per_country`, so the name says which newest it means.
- [x] Test that a later extract of one country leaves the other's points placeable, and that a
      country extracted from an older release places its points the same.
- [x] Test that a second extract of one country supersedes the first rather than adding its
      areas beside them.
- [x] Make a bare `extract` backfill every extract the manifest records, each from its own
      recorded release, rather than the newest alone. `backfill <id>` keeps taking one. It
      fills them oldest first, in the order they were taken.
- [x] Skip an extract whose rows are already in the store rather than failing on it, so filling
      in a part-filled store takes one command. `AlreadyPresent` stays the answer to a named
      id, renamed `AlreadyFilled` to match the `is_filled` the skip asks.
- [x] Report what a backfill filled in and what it skipped, so a run over several extracts says
      which of them the store now holds.
- [x] Check `just summarise` shows every extract the store holds, so what the store covers
      takes one command rather than a manifest query. It names both ends of the span and the
      count; `just summarise --partitions` lists each one, which is what to reach for past
      two.

#### Refactors / extensions: the UK as a country

- [x] Bump `proj4rs` to 0.2 in the workspace `Cargo.toml`, and check the pinned Berlin
      projection is unchanged. Nothing else in the slice waits on it. It resolves
      `crs-definitions` 0.5.0, as the decision expected, and Berlin still projects to the
      pinned metre.
- [x] Read `division_area` for the UK from the mirror at release 2026-07-22.0, and record the
      code it carries and the bbox of the `country` row. Decides the variant's code and the
      window the extract takes. The user ran it: `/Volumes` answers `Operation not permitted`
      to Claude, sandbox disabled included, so the mirror is unreadable from a session.
- [x] Add `Country::UnitedKingdom` to `crates/medallion/src/country.rs`: code `GB`,
      `projected_epsg` 25830, `projected_projjson` from `etrs89_utm30n.projjson.json`, and the
      variant in `ALL`.
- [x] Add the `EPSG:25830` line to the `crs-definitions` recipe, and commit the
      `crates/medallion/src/etrs89_utm30n.projjson.json` it emits.
- [x] Pin a GB point through `Projector::for_country` in `crates/medallion/src/geo.rs`, against
      the easting and northing PROJ gives for EPSG:25830 — the check that proj4rs and the
      notebook's projection agree for this zone.

#### Refactors / extensions: a country is named by its GERS id

Proven on DE before any further UK work, as the ids are what every later read keys on.

- [x] Add a `GersId` newtype over a UUID in `crates/medallion-model/src/gers.rs`, with `new`,
      `FromStr` delegating to it, and `Display` giving the form the data carries.
- [x] Map each country to its division's GERS id in `crates/medallion-model/src/overture.rs`,
      exhaustively over `Country`, and test that every id parses and that no two countries share
      one.
- [x] Read the country areas by `division_id` rather than by `subtype` and `country`, in both the
      places that ask Overture for them: placing a point, and taking an extract's window.
- [x] Record in `docs/overture.md` what a GERS id is, which themes commit to one, that a country
      is two areas of one division, and that the window keeps the territorial waters.
- [x] Re-derive the DE silver datasets and check the sessions place as they did, on a store whose
      extract predates the change. The read is by id now; the rows are the same rows.

#### The GB extract

- [x] Mount the Overture mirror, and take the extract: `just bronze-extract --mirror <path> new
      --release 2026-07-22.0 --country GB`. Record the extract id, the rows per theme, and the
      bytes on disk.
- [x] Check `just summarise` reports both extracts and the rows each holds, and that a rebuild
      of the DE silver datasets is unchanged by GB arriving beside them. It reports three: the
      superseded DE extract has been filled in as well.

#### Reads of a country-partitioned dataset

Found by reading the store back after the two-country write, and blocking everything downstream
of it.

- [x] Pin the constraint and the shape that works in `crates/medallion/tests/two_countries.rs`:
      a dataset written for two countries is not read in one scan, and each country is read from
      its own partition.
- [x] Add `register_of_country` and `rows_of_every_country` to `crates/medallion/src/query.rs`,
      the first for a read within one country and the second for a read that wants them all.
- [x] Take the union from the partitions the dataset holds rather than from `Country::ALL`, so a
      partition under a code this build does not know is still read — which is what
      `every_country_the_store_holds_is_packed` asserts with an `FR` partition. The partition
      values come from a query, with the engine projecting them from the layout, so nothing reads
      a directory name and a store on object storage answers the same way.
- [x] Scope the reads in `session_crossings::silver::derive` to one country, and take the union in
      `crossings::silver::read` and `session_crossings::gold::choose`, which need every country
      and read no projected column.
- [x] State in `docs/medallion.md` that a geometry column carries one CRS wherever it appears, and
      what that means for reading a dataset partitioned by country.

#### Silver observations

- [x] Re-run `just silver-sessionise`, and check `unplaceable` falls to nought and the GB fixes
      land in `country=GB`. `unplaceable` counts sessions, not fixes, and stood at 8 of 49
      before the GB extract.
- [x] Re-run `just silver-motis-ingest`, and check a country with no legs writes no partition
      and does not fail.

#### The crossings notebook

- [x] Copy `v9.py` to `v10.py` and point `silver-water-crossings` at it. The cells below change
      `v10.py` alone.
- [x] Expose `division_id` through `medallion-py`, beside `projected_crs`, so the notebook keys
      its region read on the division rather than on the country label. The region union is the
      geometry every crossing is clipped against, which makes it the third place the GERS rule
      applies after placing a point and taking a window.
- [x] Replace the pinned `EXTRACT_ID` and `COUNTRY` with a country-to-extract map covering DE
      and GB, and drive the region window, rail, water and city cells from it — the two
      `country = 'DE'` literals included. The region one becomes a division id; the locality one
      stays a country code, since it selects every city in the country rather than one entity.
- [x] Project each country's geometry with its own `lookout_medallion.projected_crs(country)`,
      and carry `country` per row.
- [x] Write the union of both countries in one `write_silver`, so the sweep and the id checks
      cover both.
- [x] Add a GB bbox case to `test_cases.geojson` with a hand-counted crossing, and run
      `crossing_checks` over both countries. The three DE cases ran and passed on the
      two-country write; the Forth Bridge case was added after that run, so it is checked on the
      next one.
- [x] Run `just silver-water-crossings`, and record the crossings each country yields. The
      user's to run: duckdb installs its `spatial` extension under `~/.duckdb`, which the
      sandbox refuses, so the notebook cannot run from a session.

#### Gold and the kiosk

- [x] Re-run `just silver-session-crossings`, and record what the GB session matched.
- [x] Take the best `max_sessions` per country in `crates/session_crossings/src/gold.rs`,
      joining `session` for the country, and say per-country in the `--max-sessions` help. The
      samples come from the same per-country read, so the union read it used is gone.
- [x] Test that a country with fewer than `max_sessions` qualifying sessions contributes what
      it has, and crowds out no other country.
- [x] Run `just gold-pack-sessions` and `just kiosk-sessions`. Record the sessions kept per
      country and the bytes. Only after the crossings notebook has run.
- [x] Run `just gold-pack-crossings` and `just crossings`. Record `crossings`, `packed_bytes`
      and `json_bytes`.
- [x] Check the carried crossings against every country the store supports rather than against
      Germany's box, which the GB points fail. `Country::bounds` says where a country is,
      coarsely, and `m5-core` takes the store as a dev-dependency to read it — host-only, as its
      `predictor` fixtures already are.

#### The invariant on the device

An early read on the DE and GB set, before the release bump makes finding out expensive.
The release group below confirms it on the final set.

- [x] Build `just m5plus-build-release`, and measure the release ELF's total size and the
      rodata carrying the point set, against the recorded 764,694 and 212,256 bytes.
- [x] Flash the release build and record the per-scan microseconds at the new count, against
      the recorded 4,353 to 5,025 µs for 5,749 crossings. The user's to run: Claude cannot
      flash the device. The firmware now reports the slowest sentence of each minute, which is
      the one that scanned, so a monitor session answers it.
- [x] Decide whether the invariant holds on those two numbers. Raise a slice for a more compact
      representation only if it does not. It holds: a tenth of flash and a hundredth of the gap
      between fixes, so no slice is raised.

#### Refactors / fixes: python reads bronze through the store

`query_silver` names silver datasets alone, so a notebook wanting the rail and water an extract
holds reaches for duckdb — which is a second reader of the store's own files. `inspect_crossings.py` does
that today.

- [x] Expose every bronze dataset the model defines as a table a query can name, so
      `extract_manifest` and the telemetry datasets register by their own names.
- [x] Take `overture_extract` by theme and type, since one table cannot span themes whose columns
      differ, and register it across every extract at once: `read_parquet` takes a list of paths,
      and the rows carry the `extract_id` the extraction wrote into them, so the manifest joins to
      an extract in plain SQL. A glob in place of the extract id matches nothing — DataFusion
      globs the last path segment only.
- [x] Read no country: bronze has no zone per country, so nothing there needs scoping.
- [x] Move `inspect_crossings.py`'s rail and water reads onto it, and drop duckdb from that notebook.
- [x] Record in `docs/medallion.md` what a python read of bronze names, beside what a read of
      silver names.
- [x] Read the silver row through its own definition where a query shapes a struct by hand:
      `crossings::silver` and `session_crossings::silver` each declare a private
      `StoredCrossing`, differently, and `crates/crossings/tests/geo.rs` shows the alternative —
      flatten `WaterCrossingRow` and add only what the row has no column for, the position.
      Three types of one name, each restating part of a schema the store already declares.
- [x] Find the country in the replay runner rather than assume one. Per-country silver left the
      runner's reads behind — `water_crossing` and `session_sample` both refuse a read that names
      no country — and its tests failed. The runner now names no country of its own: it asks the
      store which countries hold a dataset, lists every country's sessions with the country beside
      each, and looks up the country a session was recorded in before replaying it.

#### Refactors / fixes: bronze fills in a country silver cannot place

Silver and gold hold only countries the store defines a zone for, and that is now enforced. Bronze
is the record of what was observed, so it has no such rule — but `Extractor::backfill` parses the
manifest row's country into `Country` and fails on a code this build has no variant for, which
makes a recorded extract unfillable for the country it was taken for.

- [ ] Fill in a recorded extract from the country code its manifest row carries, rather than from
      a `Country`. The predicates that restrict a theme want the code, and the window comes from
      the row, so nothing in a backfill needs the store to have a zone for it.
- [ ] Test that an extract recorded for a country the store has no zone for fills in, and that its
      rows land under the id the manifest gave it.
- [ ] Drop `ExtractError::UnknownCountry` if nothing raises it once the backfill reads a code.
- [ ] Record in `docs/overture.md` that an extract is taken for a country the store can place and
      filled in by the code the manifest carries, since taking one reads that country's division
      id while filling one in reads only the manifest.

Taking a *new* extract still needs a supported country: the window comes from the country
division's GERS id, which the store holds per `Country`. Reading that id from the release by code
would lift the restriction, and is a slice of its own rather than part of this fix.

#### Up to date on the latest Overture release

Last, so it moves both countries at once over work that is already proven on 2026-07-22.0.

- [ ] Hold the mirror's path once, in `tools/overture-mirror/mirror.just`, and import it from
      that dir's Justfile and from `apps/lookout/Justfile` in place of its `overture_mirror`
      literal.
- [ ] Capture the mirroring in `tools/overture-mirror/Justfile`: one recipe listing the
      releases `s3://overturemaps-us-west-2/release/` holds, one syncing a named release whole
      to the mirror with `--no-sign-request`. Reasoning in comments, as
      `tools/motis-server/Justfile` has it.
- [ ] List the bucket's releases, and name the latest. It becomes the new pin.
- [ ] Sync that release whole to the mirror, every theme. The user's to run: it needs the drive
      mounted and hours of transfer. Everything below waits on it.
- [ ] Bump `DEFAULT_RELEASE` in `crates/transport/src/overture.rs` to that release.
- [ ] Take a DE and a GB extract at it, from the mirror. Each becomes its country's newest, so
      what places a point follows with no further change.
- [ ] Point the notebook's country-to-extract map at the two new ids, and re-run `just
      silver-init`.
- [ ] Re-run `crossing_checks` over both countries, and record any test case whose count moved
      between the two releases.
- [ ] Re-run `just gold-pack-crossings`, `just crossings`, `just gold-pack-sessions` and `just
      kiosk-sessions`, and commit the packed artefacts with the versions adopted.
- [ ] Re-measure the firmware size and the scan cost on this final set, and confirm the
      invariant on those numbers.

#### Wrap-up

- [ ] State in `docs/medallion.md`, beside the rule on one projected zone per country, that a
      zone is chosen to need no datum transformation — every engine writing the column has to
      reach the same numbers, and a transformation one applies and another skips is the way
      they diverge. Carry the Ordnance Survey and PROJ references from [Rejected /
      deferred](#rejected--deferred).
- [ ] Record in `docs/overture.md` that a country is extracted on its own, that the areas
      placing a point are the union of each country's newest extract, and that countries need
      not share a release.
- [ ] Fold the measured firmware size and scan cost into the scanning section of
      `docs/device.md`.
- [ ] Record in `docs/architecture.md` that filling in bronze takes every recorded extract.
- [ ] Point `apps/lookout/README.md` at `tools/overture-mirror/` where it says the extract
      comes from a local mirror.
- [ ] Run `just test-no-docker`.

### Observations

- **A scan of both countries costs 8,949 µs, and the device carries them.** Twenty reports from
  the release build on 2026-09-29 ran 8,894 to 9,004 µs, against 4,353 to 5,025 µs for the 5,749
  crossings before — 0.872 µs per crossing against about 0.82, so the scan is linear in the count
  with a little to spare for the longer walk through flash. That is 0.89% of the one second
  between fixes, where the recorded figure was 0.5%. Stack unused fell from 9,996 bytes to 9,980,
  and free heap stands at 2.28 MB. Nothing here needs a more compact representation.
- **The firmware grows by a tenth, and the set by four fifths.** Measured on the release ELF
  built 2026-09-29: 837,490 bytes of loaded sections against the recorded 764,694, of which
  `.flash.rodata` is 269,616 against 212,256, and 7,922 bytes of static RAM against 7,882. The
  rodata's extra 57,360 bytes is the packed set going from about 69,000 bytes to 123,228, against
  8 MB of flash. The debug build flashed and booted carrying 10,268 crossings, so the Xtensa build
  minds neither the second country nor the store arriving as a dev-dependency of `m5-core`.
- **Gold keeps three German sessions and the one British one.** `just gold-pack-sessions` on
  2026-09-28 chose 4 at `--min-crossings 5 --max-sessions 3`: DE at 41, 39 and 27 crossings, GB at
  6, over 1,692 samples and 204,620 bytes. A global cap would have taken 41, 39 and 27 and left GB
  out; the per-country cap is what keeps it.
- **The device set is 10,268 points in 123,228 bytes packed, 330,512 as JSON.** Up from 5,749
  points and about 184 KB, from the two extracts `20260804T152143Z` and `20260927T172559Z`. Of the
  carried points 5,760 fall in Germany's window and 4,508 in the UK's, none outside either.
- **A silver write sweeps the countries its rows do not cover.** The gold fixture wrote DE and
  then GB in two calls to `write_geo_rows`, and the second deleted the first: a write replaces
  the dataset, so every country it is to hold goes in one call. The crossings notebook already
  follows that rule by writing both countries in one `write_silver`; here a test failed until the
  fixture did the same.
- **`inspect_crossings.py` draws the store back, a country at a time.** Seven cells: the silver crossings
  read through `query_silver` per country, a map of each country's crossings coloured by overlap
  kind, and the four bbox test cases through `crossing_checks` and `test_viz`, with the country of
  a case taken from the extract window that holds the middle of its bbox — so the Hamburg case,
  which has no crossings to infer from, still resolves. All four cases pass through it: Mannheim
  4 of 4, the horseshoe 2 of 2, Hamburg 0 of 0, and the Forth Bridge 2 of 2 on its first real
  run. Bronze is read with duckdb until the group above lands.

- **A second country made every silver dataset with a projected column unreadable.** Reading
  `water_crossing` back through the store's own reader answered `Error during planning: Different
  GeoParquet CRS for column geometry_projected`, and `just silver-session-crossings` panicked with
  the same. `session` and `session_sample` were unreadable too, `train_segment` and
  `session_crossing` were not: one holds a single country, the other no projected column. DuckDB
  read all of them without complaint, so the two engines disagree about what a mixed-CRS scan
  means. Reads are now scoped per country, and the derivation runs.
- **The GB session matched 6 crossings, and DE matched exactly what it did before.** `just
  silver-session-crossings` on 2026-09-27 derived 259 passes over 8 partitions: DE 253 passes
  across 21 sessions and 190 crossings, unchanged, and GB 6 passes from 1 session of the 8
  recorded.
- **GB yields 4,508 crossings and DE still yields 5,760, for 10,268 in one write.** Run
  2026-09-27 on `v10.py`, two partitions written and none removed. DE's count is unchanged from
  the one-country runs, and the three DE test cases pass, which together say the two-country
  rewrite left the German path alone. Every `crossing_id` and every `crossing_compact_id` is
  distinct across both countries — 10,268 of each — so four bytes still name a crossing
  uniquely at this size, as the decision expected.
- **Each partition declares its own zone, and the numbers match it.** `country=DE` declares
  EPSG:25832 for `geometry_projected` and `country=GB` declares 25830, both with CRS84 for the
  geographic column. A GB crossing near Cardiff, -3.171783, 51.477110, is stored at
  488,070.1162, 5,702,897.5190 and `cs2cs` gives the same to four decimal places, so what the
  notebook projected and what the file declares agree.
- **The Forth Bridge is the GB test case.** Two crossings at -3.3897, 56.0027, one per track
  over one `ocean` body, each about 1,537 m of overlap, on two standard-gauge segments spanning
  55.93 to 56.04. The expected count comes from the bridge carrying two tracks, as Mannheim's
  comes from four; the bbox -3.3920, 56.0000 to -3.3860, 56.0060 holds exactly those two and
  excludes the river pair 4.4 km north at Inverkeithing.
- **The GB extract is `20260927T172559Z`, taken 2026-09-27 from the mirror at release
  2026-07-22.0.** Its window is -14.015517, 49.674000 to 2.091912, 61.061001, digit for digit
  what the mirror answers for the GB division, so reading a window by division id holds against a
  release as well as against a fixture. It writes 1,373,565 rows in 867 MiB, against DE's
  3,969,823 in 1.5 GiB:

  | theme and type | GB | DE |
  | --- | --- | --- |
  | `base/water` | 1,120,122 | 3,110,307 |
  | `transportation/connector` | 112,577 | 462,655 |
  | `divisions/division` | 61,370 | 117,467 |
  | `transportation/segment` | 55,347 | 239,614 |
  | `divisions/division_area` | 24,149 | 39,780 |

  Water is 593 MiB of the 867. GB holds a third of DE's rows on a wider window: much of the
  window is sea, which carries few rows for its area, where Germany's land carries rivers, canals
  and four times the rail.
- **Three extracts in the store, and GB is the newest.** The superseded DE extract
  `20260727T193628Z` has been filled in beside `20260804T152143Z` and the GB one, so the store
  holds 9,311,586 rows in 3.8 GiB. GB being newest overall is what the first group was for: on
  the old read every German session would now be unplaceable. They place.
- **Every recorded session places, and the countries stay apart.** `just silver-sessionise` on
  2026-09-27 derived 49 sessions and 6,179 samples with `unplaceable` at nought, against 41 and
  5,980 before. DE keeps its 41 sessions and 5,980 samples at the same 363.4 KiB and 859.7 KiB,
  untouched by GB arriving beside it; GB lands 8 sessions and 199 samples of its own, projected
  through 25830. `just silver-motis-ingest` left `train_segment` at `country=DE` alone, 5,178
  rows, writing no GB partition and reporting no failure.
- **Reading the areas by division id leaves the DE sessions exactly as they were.** `just
  silver-sessionise` on 2026-09-27 derived the same 41 sessions, 5,980 samples and 13 partitions
  of each, removing none, against an extract taken long before the change. 8 sessions of the 49
  recorded are unplaceable, which is the UK trip waiting on a GB extract — `unplaceable` counts
  sessions, placed by their starting point, rather than fixes.
- **The bigger CRS definition grows every projected dataset.** The same re-derivation wrote
  `session` at 363.4 KiB against 318.3 KiB, and `session_sample` at 859.7 KiB against 814.7 KiB,
  on identical rows: the projected geometry field carries the CRS, and the regenerated definition
  names 60 datum-ensemble members where the old one named 12. That is about 3.5 KiB per partition
  file, and it lands on any dataset with a projected column as it is next rewritten.
- **GB's country area spans -14.0155, 49.6740 to 2.0919, 61.0610**, read from the mirror at
  release 2026-07-22.0 on 2026-09-27. The code is `GB`, as the decision took it. The window is
  much wider than the island: west to Rockall, north past Shetland, south to the Scillies, and
  east into the North Sea. Everything the extract takes by window rather than by country
  therefore reaches well beyond the UK — all of Ireland, and the coast from Brittany to Jutland
  — so the rail and water rows include a second country's network, as Germany's window already
  does for its neighbours.
- **Northern Ireland is inside the GB country area.** A point-in-polygon test on the mirror,
  2026-09-27, put Belfast — -5.93, 54.60 — inside both of GB's areas, so the extract covers the
  province, the Irish Sea, and the railways of the Republic that share the window. Rockall —
  -13.69, 57.60 — falls inside the maritime area alone, the land reaching no further west than
  -8.65 at St Kilda, and that is what carries the window out to -14.02.
- **proj4rs 0.2 and PROJ agree on EPSG:25830 to under a centimetre.** Edinburgh Waverley,
  -3.188267, 55.953251, projects to 488,244.04, 6,200,892.57 through both, which is what
  `geo.rs` pins. The decision's 443,797.38, 6,200,880.49 was a different point near Edinburgh,
  about 44 km west.
- **Regenerating the CRS definitions rewrites the German one too.** `just crs-definitions`
  emits 336 more lines into `etrs89_utm32n.projjson.json`, all of them further ETRS89
  realisations in the datum ensemble that the installed EPSG database knows and the committed
  file predates. The CRS is the same, so the file is held back from this commit; regenerating
  it changes the CRS metadata every projected column carries, and is worth its own.
- **The OSTN15 grid is installed on this machine**, as
  `/opt/homebrew/share/proj/uk_os_OSTN15_NTv2_OSGBtoETRS.tif`. So the divergence [Rejected /
  deferred](#rejected--deferred) predicts for EPSG:27700 would be live here: PROJ would apply
  OSTN15 where proj4rs applies the Helmert. Nothing in the store reads 27700, and 25830 needs
  no transformation.
- **This store records two DE extracts and holds one.** `20260727T193628Z` at release
  2026-06-17.0, superseded by `20260804T152143Z` at 2026-07-22.0, whose rows are the 1.5 GiB
  `overture_extract` holds. A bare `just bronze-init` now fills the older one in as well, which
  reads a release that has aged out of the public bucket and so needs it mirrored, and costs
  another 1.5 GiB for areas nothing places a point with — the union takes each country's newest.
  Filling each country's newest alone would avoid both. Left as the task states it, since
  bronze's immutability rests on a recorded extract being present.
