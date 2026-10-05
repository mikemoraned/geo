# Next Slices

# Milestone: V1 MVP

## Target

This has a working but minimal prediction framework that has measurable predictability based on
observed data from motis. It runs on Web and on the M5Plus. It can give notifications visually on
Web/M5Plus (in a way suitable to platform capabilities).

It is ok if the predictor is a simple variation on a Crow Flies predictor that takes into account
your speed and distance from crossings.

It need only support UK and Germany.

## Slice: up to date on the latest Overture release

### Target

DE is extracted at 2026-08-04 and GB at 2026-07-22.0, while `DEFAULT_RELEASE` says
2026-06-17.0. At the end one release — the latest the bucket serves — holds both extracts, and
silver, gold, the packed device set and the kiosk sessions all derive from them. Both move at
once, so a crossing count differing between countries differs by geography rather than by
release. The mirroring becomes recipes rather than a path typed into one Justfile.

### Decisions

- **The mirror takes a whole release, not the themes an extract reads.** It serves work
  outside this repo as well, and an unanticipated theme is what a mirror exists to make
  reachable. Syncing `base/water`, `transportation` and `divisions` alone would cost a fraction
  of the hundreds of GB, and a question reaching past them would send the sync back to the
  bucket — by which point the release may have aged out.
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

### Tasks

#### The mirror, held in one place

- [ ] Hold the mirror's path once, in `tools/overture-mirror/mirror.just`, and import it from
      that dir's Justfile and from `apps/lookout/Justfile` in place of its `overture_mirror`
      literal.
- [ ] Capture the mirroring in `tools/overture-mirror/Justfile`: one recipe listing the
      releases `s3://overturemaps-us-west-2/release/` holds, one syncing a named release whole
      to the mirror with `--no-sign-request`. Reasoning in comments, as
      `tools/motis-server/Justfile` has it.
- [ ] Point `apps/lookout/README.md` at `tools/overture-mirror/` where it says the extract
      comes from a local mirror.

#### The release pin

- [ ] List the bucket's releases, and name the latest. It becomes the new pin.
- [ ] Sync that release whole to the mirror, every theme. The user's to run: it needs the drive
      mounted and hours of transfer. Everything below waits on it.
- [ ] Bump `DEFAULT_RELEASE` in `crates/transport/src/overture.rs` to that release.
- [ ] Take a DE and a GB extract at it, from the mirror; each becomes its country's newest.

#### Re-derive, re-pack, and re-measure

- [ ] Point the notebook's country-to-extract map at the two new ids, and re-run `just
      silver-init`.
- [ ] Re-run `crossing_checks` over both countries, and record any test case whose count moved
      between the two releases.
- [ ] Re-run `just gold-pack-crossings`, `just crossings`, `just gold-pack-sessions` and `just
      kiosk-sessions`, and commit the packed artefacts with the versions adopted.
- [ ] Re-measure the firmware size and the scan cost on the final set, and confirm the device
      still carries it.

#### Open questions

- Whether the latest release still carries the columns the predicates name — `subtype`,
  `class`, `connectors`, `bbox`, `country`, `division_id`. The first extract at the new pin
  answers it; a column that moved turns the bump into schema work.

# Milestone: V2

## Target

- Runs on Web/M5Plus + also: Android/iOS App + also: CoreS3 M5 device with GNSS base.
- Has a predictor that takes into account reachability of a crossing and not just distance,
  either through observational data, or through route connectivity.

## Slice: rail track geometry from pfaedle (parked)

### Target

Give rail legs real curved geometry instead of the straight stop-to-stop lines DELFI's
`shapes.txt` yields for rail — see [motis.md](motis.md). pfaedle map-matches GTFS trips onto
OSM to synthesise `shapes.txt`, and produced correct curved rail: `-D -m rail` recomputes
rail shapes only, leaving bus and coach shapes alone, and rail polylines come out hundreds
of points where they were four.

**Parked, and the tooling was reverted out of the tree** (`tools/pfaedle`, commit
`dfd8655`), because importing the result breaks realtime.

### Why it is parked

Import the raw DELFI feed and around 99.97% of RT entities resolve. Import any feed carrying
pfaedle's `shapes.txt` and trip resolution fails for ~99.6% of them, with **no segment coming
back realtime-corrected**. The static schedule itself imports fine: the trips are there and
the rail is genuinely curved.

It is the `shapes.txt` and not the trips. Three attempts broke realtime identically,
including one that kept `trips.txt` byte-identical to the raw feed apart from the rail
`shape_id` fields — and there the failing trips were *bus* trips whose `shape_id` was never
touched. The only remaining difference is the swapped-in `shapes.txt`, which grows from
308 MB to 2.3 GB. It is not feed currency either: a same-day RT fetch still overlapped the
static feed's trip ids 99.6%.

Leading hypothesis, untested: the 2.3 GB of rail geometry makes `motis import` hit some
limit and produce a timetable whose RT trip index is incomplete, while scheduled queries
still work.

### To resume

1. Confirm the trigger — build the raw feed with only `shapes.txt` swapped, import, and check
   the RT statistic. Expect it to break.
2. Chase the cause: read `motis import` for shape, memory or limit warnings; try shrinking
   `shapes.txt`, by simplifying the rail polylines or dropping the unused bus shapes, and
   re-test.
3. If Motis genuinely cannot take large rail shapes, file an issue upstream, or accept
   straight-line rail — which is what transitous does — and drop this.

Two facts about pfaedle worth keeping if it resumes: it has no homebrew formula and has to
be built from source against `cmake` and `libzip`, and it must run from its build directory
with an explicit config path, since it only finds its default MOT-to-OSM matching config
when installed. Its GTFS parser is also stricter than Motis's — it aborts on the dangling
references in DELFI's `transfers.txt` and `pathways.txt`, which Motis tolerates.

Each realtime A/B needs the Motis server run by hand: the sandbox denies the LMDB tile mmap.

# Unassigned

Anything that doesn't clearly fit in V1 or V2.

## Slice: make the store operable at size

### Target

The store's layout is settled; what it lacks is the ability to be *worked* — to re-derive
part of history rather than all of it, and to stop accumulating files without bound. Both
become urgent at a size we are not at yet, and both are cheaper to build before then.

### Refactors / extensions

- **Give the derivation CLIs a date-range argument**, so a run can ask for less than
  everything. They currently read every partition and filter on data columns, which means
  the partition pruning the layout provides is never exercised: re-deriving one day's output
  costs a full scan. This is also the prerequisite for handing the work to an orchestrator
  later, since a range is what a backfill is expressed in.
- **Write down a compaction plan for the append-shaped layers**, before the small-file
  problem is real rather than after. One file per ingestion is deliberate and correct at the
  point of writing, but a dataset polled on an interval accumulates a file per poll
  indefinitely (the sqlite backfill alone produced 1,307 in one dataset). The standard answer
  is periodic compaction into fewer, larger files per partition; the thing to decide is what
  triggers it and how it preserves immutability, since rewriting files is what that layer
  forbids.
- **Leave the engine catalog traits alone** until registering datasets by hand is genuinely
  annoying, then add a schema provider *over* the dataset definitions rather than replacing
  them. The definitions are plain data every engine can read; a catalog is one engine's view
  of it, and those traits move between that engine's releases.

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. That allows a recommendation about which direction to look from the train seat.

## Slice: Adding POIs from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POIs by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to identify exactly what the POI is based on the image.

## Slice: SedonaDB 0.5.0 and the arrow generation

### Target

SedonaDB 0.5.0 moves arrow and parquet to 58.3, datafusion to 54.1, and object_store to 0.13.
Every crate the store shares a `RecordBatch` with moves with them. The move fixes nothing known:
0.5.0-rc0 still has the panic in
[2026-10-03-sedona-nested-column-panic.md](2026-10-03-sedona-nested-column-panic.md). It keeps the
pins current, and gets the arrow generation off 57 before more code depends on it.

### Decisions

- **The move waits for a release.** 0.5.0-rc0 was tagged on 2026-10-02. A pin to a release
  candidate gives up the reproducibility the pin exists for.
- **arrow 58 moves the geo crates with it.** geoparquet, geoarrow-schema, and geoarrow-array go
  to 0.8, pyo3-arrow to 0.17, and serde_arrow to 0.14 with its `arrow-58` feature. None of them
  builds against arrow 58 alone, so they share one commit. serde_arrow 0.13 has no `arrow-58`
  feature. The workspace comment in `Cargo.toml` is wrong that geoparquet 0.8 moved to arrow 59:
  0.8 builds on 58.
- **geo moves to 0.33, the version `sedona-geo` resolves at 0.5.0-rc0.** `crates/domain` and
  `crates/predictor` pin `geo` themselves, at 0.32, the newest beside SedonaDB 0.4.1. Until they
  move too, the lock fails. The device build depends on both, so the move needs a device build.

### Tasks

- [ ] Move `sedona` and `sedona-geoparquet` to the 0.5.0 tag, `arrow` and `parquet` to 58.3,
      `datafusion` to 54.1, and `object_store` to 0.13.
- [ ] In the same commit, move `geoparquet`, `geoarrow-schema`, and `geoarrow-array` to 0.8,
      `pyo3-arrow` to 0.17, and `serde_arrow` to 0.14 with its `arrow-58` feature.
- [ ] Move `geo` to 0.33 in the workspace, in `crates/domain`, and in `crates/predictor`.
- [ ] Rewrite the arrow comment in `Cargo.toml` for arrow 58, dropping its claim about
      geoparquet 0.8.
- [ ] Build with `just m5plus-build-release`, since the device shares `domain` and `predictor`.
- [ ] Run `just test-no-docker` and `just test-geo`.

### Observations

- 2026-10-03: a scratch copy of the app built against 0.5.0-rc0 and the versions above with no
  source change. 490 non-Docker tests and the geo tests passed. The device build was not tried.

## Slice: the writing rules applied at the edit, not at the stop

### Target

The writing skills hold the rules, and the prose gate enforces only that the skills were invoked.
On 2026-10-03 a session invoked both before drafting, skipped the technical-writing self-check,
and reported the prose done. The self-check, run on request afterwards, found four sentences over
the cap and four conditions after their clause. It also found a colloquial term and a claim nobody
had verified.
The gate passed that session at every stop, for five reasons:

- **It enforces the invocation, not the work.** A `Skill` call naming each skill satisfies it, and
  nothing in it reads the prose.
- **It counts invocations across the whole session.** One invocation early in a session
  satisfies every later stop. A second invocation returns "already loaded" and shows no rules.
- **It fires once, after the reply.** The Stop hook runs after the reply already reports the work
  done, and `stop_hook_active` limits it to one block per turn.
- **It misses edits made through Bash.** Most of that session's prose went in through `python3`
  and `sed`.
- **It reads `*.md` alone.** Comments in `*.rs` files and `Justfile` recipe help escape it.

At the end the rules reach Claude at the edit that needs them. Until both skills are invoked in
the current turn, an edit to a prose file is refused. Every change to a prose file, through any
tool, puts the self-check in front of Claude before its next step.

### Decisions

- **No prose linter.** The skills already state the rules, and a linter states them a second time.
  The failure is in applying the rules, so the hooks enforce the application.
- **A PreToolUse hook on `Edit|Write` refuses an edit to a prose file.** If the transcript holds no
  `Skill` call for each writing skill since the last user message, the hook returns
  `permissionDecision: "deny"`. Its reason names the missing skills, and the edit does not run.
- **A PostToolUse hook on `Edit|Write` returns the self-check.** Its reason names the file changed
  and carries the technical-writing self-check. It asks Claude to apply the self-check to the lines
  written and report the result before the next step.
- **A PostToolUse hook on `Bash` covers changes made through scripts.** It compares
  `git status -- '*.md'` with the state it last recorded for the session. If a prose file
  changed, it returns the same self-check.
- **The Stop gate stays as the backstop**, and counts only the invocations since the last user
  message.
- **The hooks are modes of `tools/prose-gate`**, which already parses the transcript. Each mode is
  a subcommand, and `.claude/settings.json` wires each to its event.

### Open questions

- Which fields a PostToolUse hook returns for Claude to read. The PreToolUse `permissionDecision`
  field is known. The PostToolUse output is not yet compared with the hook documentation.
- Where the self-check text comes from. A copy in `tools/prose-gate` drifts from the skill. A
  read of the skill's `SKILL.md` at run time depends on the plugin cache path.
- Whether comments in `*.rs` files and `Justfile` recipe help count as prose for all three
  hooks, or for the Stop gate alone.
- Whether this slice belongs to lookout. The gate serves the whole repo, and the slice lives
  here only because lookout is where the failure happened.

### Tasks

#### The turn, not the session

- [ ] Find the last user message in a transcript, and count only the `Skill` calls after it.
      A tool result arrives as a user message too, so the search skips those.
- [ ] Apply that count in the Stop gate. Test it on a session whose only invocations sit in an
      earlier turn.

#### At the edit

- [ ] Add a `pre-edit` subcommand that reads the PreToolUse payload. When a writing skill has no
      invocation in the current turn, deny an edit to a prose file.
- [ ] Add a `post-edit` subcommand: return the self-check for the file the payload names.
- [ ] Add a `post-bash` subcommand that compares the prose files' git state with the state last
      recorded for the session. When a prose file changed, return the self-check.
- [ ] Wire the three subcommands in `.claude/settings.json`.
- [ ] Describe the three hooks in `tools/prose-gate/README.md`, and the turn-scoped count.

#### Proof

- [ ] Start a session, edit a `.md` file without invoking the skills, and confirm the edit is
      refused.
- [ ] Invoke both skills, edit the file, and confirm the self-check arrives before the next step.
- [ ] Change the file through Bash, and confirm the self-check arrives there as well.
