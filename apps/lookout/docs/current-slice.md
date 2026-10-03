# Current Slice: up to date on the latest Overture release

### Target

Both DE and GB are extracted at 2026-07-22.0, while `DEFAULT_RELEASE` says 2026-06-17.0. At
the end one release — the latest the bucket serves — holds both extracts, and silver, gold, the
packed device set and the kiosk sessions all derive from them. Both move at once, so a crossing
count differing between countries differs by geography rather than by release. The mirroring becomes recipes rather than a path typed into one Justfile.


### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

- **The mirror takes a whole release, not the themes an extract reads.** It serves work
  outside this repo as well, and an unanticipated theme is what a mirror exists to make
  reachable. 2026-09-23.1 is 619 GB whole. `base/water`, `transportation` and `divisions`
  together are 131 GB. A question reaching past those three would send the sync back to the
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
- **The crossing-check baseline is taken before silver is re-derived.** `just crossing-checks`
  reads the silver the store holds, and `just silver-init` overwrites it. Without the baseline
  there are no old counts to compare against.

#### Consequences elsewhere

- Ids in silver `session_crossing`, the packed crossings and the kiosk sessions change for
  every crossing whose water changed. All three are re-derived in this slice, so nothing in
  the store refers to an id that no longer exists.

### Tasks

#### The mirror, held in one place

- [ ] Hold the mirror's path once, in `tools/overture-mirror/mirror.just`, and import it from
      that dir's Justfile and from `apps/lookout/Justfile` in place of its `overture_mirror`
      literal.
- [ ] Capture the mirroring in `tools/overture-mirror/Justfile`: one recipe listing the
      releases `s3://overturemaps-us-west-2/release/` holds, one syncing a named release whole
      to the mirror with `--no-sign-request`. Reasoning in comments, as
      `tools/motis-server/Justfile` has it.
- [ ] Make the listing recipe mark each release a later `.N` of the same date supersedes, and
      make the sync recipe refuse one.
- [ ] State in the "A release is immutable" section of `docs/overture.md` that a later `.N`
      of the same date replaces the earlier release.
- [ ] Point `apps/lookout/README.md` at `tools/overture-mirror/` where it says the extract
      comes from a local mirror.

#### The release pin

- [x] List the bucket's releases, and name the latest. It becomes the new pin.
- [ ] Confirm the mirror drive has 619 GB free. The user's to check: the sandbox cannot read
      the drive.
- [ ] Sync 2026-09-23.1 whole to the mirror with the new recipe. The user's to run: it needs
      the drive mounted and hours of transfer. Every task after this one waits on it.
- [ ] Bump `DEFAULT_RELEASE` in `crates/transport/src/overture.rs` to 2026-09-23.1.
- [ ] Take a DE and a GB extract with `just bronze-extract new`, from the mirror. Each becomes
      its country's newest.

#### Re-derive, re-pack, and re-measure

- [ ] Run `just crossing-checks` on the current silver, and keep its output as the baseline.
      It must run before `just silver-init`.
- [ ] Point `EXTRACTS` in `notebooks/water_crossings/v10.py` at the two new ids, and re-run
      `just silver-init`.
- [ ] Re-run `just crossing-checks`, and record each test case whose count moved from the
      baseline.
- [ ] Re-run `just gold-pack-crossings`, `just crossings`, `just gold-pack-sessions` and `just
      kiosk-sessions`, and commit the packed artefacts with the versions adopted.
- [ ] Count the crossing ids shared by gold crossings `20260928T215057796Z` and the new
      version, and record the share under Observations.
- [ ] Re-measure the firmware size and the scan cost on the final set, and confirm the device
      still carries it.

#### Wrap-up

- [ ] Run `just test-no-docker` and `just test-geo`.

#### Open questions

- Whether the nested columns kept their shape. 2026-09-23.1 carries every column the
  predicates name, at the top level. A changed struct inside `connectors` or `bbox` shows up
  only at the first extract, and turns the bump into schema work.

### Observations

- 2026-10-03: the manifest records both newest extracts at release 2026-07-22.0. DE
  `20260804T152143Z` was taken on 2026-08-04, and GB `20260927T172559Z` on 2026-09-27. The
  countries already share a release, so this slice moves both from one shared release to the
  next.
- 2026-10-03: 2026-09-23.1 is 619 GB in 1,278 objects. `base/water` is 29 GB,
  `transportation` 97 GB and `divisions` 6 GB.
- 2026-10-03: the parquet schemas of 2026-09-23.1 carry `subtype`, `class` and `bbox` on water.
  Segment carries `connectors`, and division_area carries `country` and `division_id`.
