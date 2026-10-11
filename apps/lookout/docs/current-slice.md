# Current Slice: up to date on the latest Overture release

### Target

Both DE and GB are extracted at 2026-07-22.0, while `DEFAULT_RELEASE` says 2026-06-17.0. At
the end one release — the latest the bucket serves — holds both extracts, and silver, gold, the
packed device set and the kiosk sessions all derive from them. Both move at once, so a crossing
count differing between countries differs by geography rather than by release. The mirroring
becomes recipes rather than a path typed into one Justfile.


### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

- **The mirror takes a whole release, not the themes an extract reads.** It serves work
  outside this repo as well, and an unanticipated theme is what a mirror exists to make
  reachable. 2026-09-23.1 is 577 GiB whole. `base/water`, `transportation` and `divisions`
  together are 122 GiB. A question reaching past those three would send the sync back to the
  bucket, by which point the release can have aged out.
- **The mirroring recipes live at `tools/overture-mirror/`.** Anything in the repo that reads
  Overture uses the mirror, not lookout alone, so it sits beside `tools/motis-server` and
  follows it: a Justfile carrying the fetch, with the reasoning in its comments. The mirror
  itself stays on an external drive — the repo records how it is made, not what it holds. A
  recipe in `apps/lookout/Justfile` would tie a shared prerequisite to one app.
- **`tools/overture-mirror/mirror.just` states where the mirror is, once**, imported by that
  dir's Justfile and by `apps/lookout/Justfile` in place of its `overture_mirror` literal. A
  variable-only file adds no recipes to whatever imports it, and an importing Justfile's
  recipes still run from its own directory, so `just bronze-init` is unchanged — checked
  against just 1.46. The path moves with the drive, which would otherwise leave one of the two
  Justfiles pointing at nothing.
- **The pin is 2026-09-23.1.** It is the newest of the three releases the bucket serves, beside
  2026-08-19.0 and 2026-09-23.0.
- **A superseded release is never mirrored or pinned.** A later `.N` of the same date means
  Overture found a fault in the earlier one, so 2026-09-23.1 replaces 2026-09-23.0 outright. An
  extract already taken from a superseded release stays in use. If 2026-09-23.1 breaks an
  extract, the fault is fixed against 2026-09-23.1.
- **The slice counts the crossing ids that survive the bump.** A water id is derived from the
  feature, so a crossing id changes wherever its water changed. The count tells the evaluation
  slice how far ground truth recorded at one release carries to the next. The two gold
  versions hold the old and new ids, so the count needs no snapshot of silver. Skipping it
  leaves the question to the first slice that needs ids to persist.
- **A signature decides which files a resumed sync still needs.** A file's signature is its
  size, its first 1 KiB, and its last 1 KiB. A local file whose signature matches the remote
  file's is complete, and every other file is copied again. A copy of a whole release takes
  hours, and a copy that stops part-way restarts from what is already on the drive. Reading the
  remote halves takes two ranged reads per object, about 2,600 for 2026-09-23.1's 1,278.
- **The tool reads the bucket through `object_store`, not the AWS CLI.** The CLI starts a
  process for each ranged read, about 2,600 per release. `object_store` makes one client for the
  run, retries a failed request, and offers an in-memory store the tests run against. lookout
  already depends on it. Confirmed 2026-10-04.
- **`verify` applies the same signature check to a mirrored release**, with no copy. It reports
  each file missing locally, differing from the remote, or present locally alone. It confirms an
  existing mirror is complete without a second download.
- **A source is the bucket or a mirror, and a destination is always a mirror.** `releases`,
  `sync` and `verify` take a source as `s3://<bucket>/<prefix>` or a local path. The user keeps
  a mirror on a portable disk and backs it up to a second mirror on a NAS. `sync` from the
  portable mirror makes the backup, and `verify` confirms the two match. A local source reads
  through `object_store`'s local file store, so signatures and copying work unchanged. The
  recipes default the source to the Overture bucket and the destination to `overture_mirror`.
  Confirmed 2026-10-04.
- **The recipes take the release, then the source, then the destination.** The order reads from
  source to destination. It moves `verify`'s second argument from the mirror to the source.
- **A source's directories that are not releases, and its `.part` files, are skipped.** A NAS or
  an operating system can add directories of its own beside the releases. A `.part` file is an
  unfinished copy, and a destination never takes one.
- **Progress and time remaining count the bytes a command reads from the source.** A release's
  files vary widely in size, so a count of files predicts a copy's end poorly. `sync` counts each
  copied file's size. `verify` counts the signature bytes it reads, at most 2 KiB a file. A file
  a command reads nothing for leaves the total. Confirmed 2026-10-04.
- **Files named in a denylist are ignored by `sync` and `verify`.** It holds `.DS_Store`, which
  Finder writes into a mirror's directories. Copying one, or reporting one as present locally
  alone, made a mirror fail `verify`. Confirmed 2026-10-04.
- **`indicatif` draws the spinner and the bars.** It offers spinners, several bars at once, and
  a time remaining for each bar, so nothing of it is hand-rolled. Confirmed 2026-10-04.
- **The rate and time left cover the last 60 seconds.** indicatif's own estimate gives data
  older than 15 seconds a tenth of its weight. A whole-file copy advances the bar by a whole file
  at once, often more than 15 seconds apart, so that estimate spikes as each file completes and
  decays between them. A 60-second window spans several files, so it holds steady. The bar's own
  fields compute it, through indicatif's `ProgressTracker`. Reporting a copy's bytes as they
  arrive would also smooth the input, and stays open if the window is not enough. Confirmed
  2026-10-11.
- **The crossing-check baseline is taken before silver is re-derived.** `just test-geo`
  reads the silver the store holds, and `just silver-init` overwrites it. Without the baseline
  there are no old counts to compare against.

#### Consequences elsewhere

- Ids in silver `session_crossing`, the packed crossings and the kiosk sessions change for
  every crossing whose water changed. All three are re-derived in this slice, so nothing in
  the store refers to an id that no longer exists.

### Tasks

#### The mirror, held in one place

- [x] Hold the mirror's path once, in `tools/overture-mirror/mirror.just`, and import it from
      that dir's Justfile and from `apps/lookout/Justfile` in place of its `overture_mirror`
      literal.
- [x] Capture the mirroring in `tools/overture-mirror/Justfile`: one recipe listing the
      releases `s3://overturemaps-us-west-2/release/` holds, one syncing a named release whole
      to the mirror with `--no-sign-request`. Reasoning in comments, as
      `tools/motis-server/Justfile` has it.
      The reasoning went in `tools/overture-mirror/README.md` instead, since the comment rule
      limits a recipe comment to its one-line help.
- [x] Make the listing recipe mark each release a later `.N` of the same date supersedes, and
      make the sync recipe refuse one.
- [x] State in the "A release is immutable" section of `docs/overture.md` that a later `.N`
      of the same date replaces the earlier release.
- [x] Point `apps/lookout/README.md` at `tools/overture-mirror/` where it says the extract
      comes from a local mirror.
- [x] Compute a file's signature in `tools/overture-mirror`, locally and from the bucket by
      ranged reads.
- [x] Make `sync` resumable: compare signatures, and copy only the files missing or differing
      locally.
- [x] Add a `verify` subcommand and recipe, reporting each file missing, differing, or present
      locally alone.
- [x] Update `tools/overture-mirror/README.md` for resuming and for `verify`.

#### A mirror as a source

- [x] Generalise `Bucket` into `Source` in `tools/overture-mirror/src/mirror.rs`: an object store,
      and the prefix its releases sit under.
- [x] Parse a source from `s3://<bucket>/<prefix>` or a local path.
- [x] Skip a source's non-release directories in `releases`, and its `.part` files in `sync` and
      `verify`.
- [x] Add `--source` to `releases`, `sync` and `verify`, defaulting to the Overture bucket.
- [x] Give each recipe a `source` argument defaulting to the bucket, and `sync` a `mirror`
      argument defaulting to `overture_mirror`.
- [x] Describe verifying and backing up a second mirror in `tools/overture-mirror/README.md`.
- [x] Copy a file from a mirror source whole, by a file copy, not in object_store's 8 KiB
      stream chunks. A release from a mirror takes about 75 million chunked reads, each a
      blocking call, which caps a disk-to-disk copy below the disks' speed.

#### Progress for long runs

- [x] Show `sync` and `verify` as still active, with a spinner that ticks while files are in
      flight.
- [x] Show a progress bar of completed bytes for each theme, and one for the release overall.
      One bar covers the release. A bar per theme overflowed a terminal with fewer rows than the
      release has themes and types.
- [x] Show the predicted time remaining for the release, and for each theme.
      The time is shown for the release alone, as the bar is.
- [x] Split `sync` into a checking stage and a copying stage, each with its own bar, sharing
      `verify`'s check.
- [x] Show the rate and the time left over the last 60 seconds, as fields of the bar's style in
      `tools/overture-mirror/src/progress.rs`.

#### The release pin

- [x] List the bucket's releases, and name the latest. It becomes the new pin.
- [x] Confirm the mirror drive has 577 GiB free. The user's to check, with the drive mounted.
- [x] Sync 2026-09-23.1 whole to the mirror with the new recipe. The user's to run: it needs
      the drive mounted and hours of transfer. Every task after this one waits on it.
- [-] Bump `DEFAULT_RELEASE` in `crates/transport/src/overture.rs` to 2026-09-23.1.
      Moot: the next task removes `DEFAULT_RELEASE`.
- [x] Replace `DEFAULT_RELEASE` with an enum of two variants, `LatestReleaseAvailable` and
      `NamedRelease(<version>)`. `extract new` without `--release` takes
      `LatestReleaseAvailable`, which resolves to the latest release the source holds.
- [x] Take a DE and a GB extract with `just bronze-extract new`, from the mirror. Each becomes
      its country's newest.
      DE is `20261011T154538Z` and GB is `20261011T154005Z`.

#### Re-derive, re-pack, and re-measure

- [x] Replace `just crossing-checks` with `just test-geo`. Both check the same silver against
      the same cases, but only `crossing-checks` reports a passing case's detail. Make each
      case in `crates/crossings/tests/geo.rs` print its expected count, its found count and
      its country, and make the `geo` nextest profile show a passing test's output. Remove
      the recipe and `notebooks/water_crossings/check_cases.py`, and name `just test-geo`
      wherever this slice names `just crossing-checks`.
- [x] Run `just test-geo` on the current silver, and keep its output as the baseline.
      It must run before `just silver-init`.
- [x] Point `EXTRACTS` in `notebooks/water_crossings/v10.py` at the two new ids, and re-run
      `just silver-init`.
- [x] Re-run `just test-geo`, and record each test case whose count moved from the
      baseline.
- [x] Re-run `just gold-pack-crossings`, `just crossings`, `just gold-pack-sessions` and `just
      kiosk-sessions`, and commit the packed artefacts with the versions adopted.
      Crossings are `20261011T172420832Z` and sessions `20261011T172440982Z`.
- [x] Count the crossing ids shared by gold crossings `20260928T215057796Z` and the new
      version, and record the share under Observations.
- [x] Re-measure the firmware size and the scan cost on the final set, and confirm the device
      still carries it.
      The scan cost was not re-measured. The set fell from 10,268 crossings to 10,254, and
      `docs/device.md` records the scan as linear in the count.

#### Refactors / improvements

- [ ] Rebuild `lookout-medallion` only when its sources change. List the files it builds from
      as `cache-keys` in `crates/medallion-py/pyproject.toml`: its own `pyproject.toml`,
      `Cargo.toml` and `src/**/*.rs`, the same of `medallion`, `medallion-model` and `domain`,
      and the workspace's `Cargo.toml` and `Cargo.lock`. Drop `--reinstall-package
      lookout-medallion` from `silver-water-crossings`, `test-python` and
      `crates/medallion-py/README.md`. The README states that a crate added to its
      dependencies joins the list, and that `--reinstall-package` still forces a rebuild.
- [ ] Do the same for `lookout-predictor` in `crates/platform/rerun-py`.

#### Wrap-up

- [ ] Run `just test-no-docker` and `just test-geo`.

#### Open questions

- Whether the nested columns kept their shape. 2026-09-23.1 carries every column the
  predicates name, at the top level. A changed struct inside `connectors` or `bbox` shows up
  only at the first extract, and turns the bump into schema work.
  Resolved 2026-10-11: they kept it. The extract reads each segment's `connectors` and every
  type's `bbox` fields, and `v10.py` reads the same, and both ran on 2026-09-23.1 without
  error. No schema work follows.

### Observations

- 2026-10-03: the manifest records both newest extracts at release 2026-07-22.0. DE
  `20260804T152143Z` was taken on 2026-08-04, and GB `20260927T172559Z` on 2026-09-27. The
  countries already share a release, so this slice moves both from one shared release to the
  next.
- 2026-10-03: 2026-09-23.1 is 577 GiB in 1,278 objects. `base/water` is 26.6 GiB,
  `transportation` 90.3 GiB and `divisions` 5.2 GiB.
- 2026-10-03: the parquet schemas of 2026-09-23.1 carry `subtype`, `class` and `bbox` on water.
  Segment carries `connectors`, and division_area carries `country` and `division_id`.
- 2026-10-11: the crossing-check baseline, from `just test-geo` on silver derived at
  2026-07-22.0. Every case passes, and each finds the count it expects.

  | Case                  | Country | Expected | Found |
  |-----------------------|---------|----------|-------|
  | Mannheim, 4 tracks    | DE      | 4        | 4     |
  | Horseshoe re-crossing | DE      | 2        | 2     |
  | Hamburg tunnel        | DE      | 0        | 0     |
  | Forth Bridge          | GB      | 2        | 2     |
- 2026-10-11: silver re-derived from the 2026-09-23.1 extracts. Every case in `just test-geo`
  finds the count of the baseline, so no case moved. `water_crossing` holds 10,254 rows
  against 10,268 at 2026-07-22.0, and `session_crossing` holds 259 rows at both. The
  derivation read the nested columns without error, which settles the open question.
- 2026-10-11: 8,804 of the 10,268 crossings at 2026-07-22.0, 86%, keep their id at
  2026-09-23.1. The counts compare the compact ids of gold crossings `20260928T215057796Z` and
  `20261011T172420832Z`.

  | Crossing at 2026-07-22.0     | Count  |
  |------------------------------|--------|
  | Id present at 2026-09-23.1   | 8,804  |
  | Id absent at 2026-09-23.1    | 1,464  |
  | Total                        | 10,268 |

  | Crossing at 2026-09-23.1                    | Count  |
  |---------------------------------------------|--------|
  | Id present at 2026-07-22.0                  | 8,804  |
  | New id, at a position 2026-07-22.0 held     | 849    |
  | New id, at a position 2026-07-22.0 lacked   | 601    |
  | Total                                       | 10,254 |

  3 of the 8,804 kept their id at a changed position. A crossing id joins four parts: the
  water's id, the track's id, the rail segment's id, and the crossing's fraction along that
  segment. The track's id is the smallest segment id in the track. A change to any part gives a
  new id at an unchanged position, so the 849 can come from a new water id, a segment added to
  or removed from the track, a new segment id, or a segment reshaped about the crossing. A
  reshaped segment can equally move a crossing and keep its fraction, which would explain the
  3. These counts do not say which part changed.
- 2026-10-11: the release firmware is 850,832 bytes against 8 MB of flash, carrying the
  123,060-byte set. Built from the same code with the previous set, it is 850,992 bytes. The
  `.flash.rodata` section that holds the set fell from 262,608 bytes to 262,448, and static RAM
  stayed at 7,922 bytes. Sizes are from `xtensa-esp32-elf-size` on the release ELF.
