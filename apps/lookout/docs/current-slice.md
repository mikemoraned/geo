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
- **Partitioning geo silver by country needs no work.** The country level is applied above a
  dataset's own partition key by the shared silver write path, not declared per dataset, so a
  second country lands in its own partitions and its own CRS as soon as `Country` knows it.
  `match_crossings` already sweeps `Country::ALL`, and so follows too.

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
- Whether the one recorded UK session — 402 fixes from Glasgow to Edinburgh, ingested
  2026-09-18 — passes five crossings and so reaches the kiosk. If it does not, either
  `--min-crossings` drops or a country's best is kept whatever it passed.
- Whether Overture's GB country area includes Northern Ireland, and so how far west the
  window reaches. The boundary decides it; it changes how much water the extract holds, not
  what is derived.

### Tasks

#### Refactors / extensions: adding a country leaves the others standing

The invariant above, in place before the GB extract is taken, and testable on the DE extract
alone.

- [ ] Take the country areas from the newest extract of each country in
      `crates/transport/src/countries.rs`, registering each extract's `division_area`. A
      country with no extract contributes no areas rather than failing, since `ALL` names a
      country before its extract is taken.
- [ ] Test that a later extract of one country leaves the other's points placeable, and that a
      country extracted from an older release places its points the same.
- [ ] Test that a second extract of one country supersedes the first rather than adding its
      areas beside them.
- [ ] Make a bare `extract` backfill every extract the manifest records, each from its own
      recorded release, rather than the newest alone. `backfill <id>` keeps taking one.
- [ ] Skip an extract whose rows are already in the store rather than failing on it, so filling
      in a part-filled store takes one command. `AlreadyPresent` stays the answer to a named
      id.
- [ ] Report what a backfill filled in and what it skipped, so a run over several extracts says
      which of them the store now holds.
- [ ] Check `just summarise` shows every extract the store holds, so what the store covers
      takes one command rather than a manifest query.

#### Refactors / extensions: the UK as a country

- [ ] Bump `proj4rs` to 0.2 in the workspace `Cargo.toml`, and check the pinned Berlin
      projection is unchanged. Nothing else in the slice waits on it.
- [ ] Read `division_area` for the UK from the mirror at release 2026-07-22.0, and record the
      code it carries and the bbox of the `country` row. Decides the variant's code and the
      window the extract takes.
- [ ] Add `Country::UnitedKingdom` to `crates/medallion/src/country.rs`: code `GB`,
      `projected_epsg` 25830, `projected_projjson` from `etrs89_utm30n.projjson.json`, and the
      variant in `ALL`.
- [ ] Add the `EPSG:25830` line to the `crs-definitions` recipe, and commit the
      `crates/medallion/src/etrs89_utm30n.projjson.json` it emits.
- [ ] Pin a GB point through `Projector::for_country` in `crates/medallion/src/geo.rs`, against
      the easting and northing PROJ gives for EPSG:25830 — the check that proj4rs and the
      notebook's projection agree for this zone.

#### The GB extract

- [ ] Mount the Overture mirror, and take the extract: `just bronze-extract --mirror <path> new
      --release 2026-07-22.0 --country GB`. Record the extract id, the rows per theme, and the
      bytes on disk.
- [ ] Check `just summarise` reports both extracts and the rows each holds, and that a rebuild
      of the DE silver datasets is unchanged by GB arriving beside them.

#### Silver observations

- [ ] Re-run `just silver-sessionise`, and check `unplaceable` falls to nought and the GB fixes
      land in `country=GB`.
- [ ] Re-run `just silver-motis-ingest`, and check a country with no legs writes no partition
      and does not fail.

#### The crossings notebook

- [ ] Copy `v9.py` to `v10.py` and point `silver-water-crossings` at it. The cells below change
      `v10.py` alone.
- [ ] Replace the pinned `EXTRACT_ID` and `COUNTRY` with a country-to-extract map covering DE
      and GB, and drive the region window, rail, water and city cells from it — the two
      `country = 'DE'` literals included.
- [ ] Project each country's geometry with its own `lookout_medallion.projected_crs(country)`,
      and carry `country` per row.
- [ ] Write the union of both countries in one `write_silver`, so the sweep and the id checks
      cover both.
- [ ] Add a GB bbox case to `test_cases.geojson` with a hand-counted crossing, and run
      `crossing_checks` over both countries.
- [ ] Run `just silver-water-crossings`, and record the crossings each country yields.

#### Gold and the kiosk

- [ ] Re-run `just silver-session-crossings`, and record what the GB session matched.
- [ ] Take the best `max_sessions` per country in `crates/session_crossings/src/gold.rs`,
      joining `session` for the country, and say per-country in the `--max-sessions` help.
- [ ] Test that a country with fewer than `max_sessions` qualifying sessions contributes what
      it has, and crowds out no other country.
- [ ] Run `just gold-pack-sessions` and `just kiosk-sessions`. Record the sessions kept per
      country and the bytes. Only after the crossings notebook has run.
- [ ] Run `just gold-pack-crossings` and `just crossings`. Record `crossings`, `packed_bytes`
      and `json_bytes`.

#### The invariant on the device

An early read on the DE and GB set, before the release bump makes finding out expensive.
The release group below confirms it on the final set.

- [ ] Build `just m5plus-build-release`, and measure the release ELF's total size and the
      rodata carrying the point set, against the recorded 764,694 and 212,256 bytes.
- [ ] Flash the release build and record the per-scan microseconds at the new count, against
      the recorded 4,353 to 5,025 µs for 5,749 crossings. The user's to run: Claude cannot
      flash the device.
- [ ] Decide whether the invariant holds on those two numbers. Raise a slice for a more compact
      representation only if it does not.

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
