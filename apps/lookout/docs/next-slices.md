# Next Slices

# Milestone: V1 MVP

## Target

This has a working but minimal prediction framework that has measurable predictability based on
observed data from motis. It runs on Web and on the M5Plus. It can give notifications visually on
Web/M5Plus (in a way suitable to platform capabilities).

It is ok if the predictor is a simple variation on a Crow Flies predictor that takes into account
your speed and distance from crossings. Where possible we shall bias towards precision over recall 
i.e. it is better to miss some crossings than notify on ones that aren't reached.

It need only support UK and Germany.

There should be some minimal display of map context to contextualise where you are when used live e.g.
the nearby trainlines and maybe the water bodies. This can be very minimal however e.g. showing the
train line is probably enough.

## Slice: Motis as ground truth for the UK

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

### Tasks

- [ ] Make the poller log a warning when a poll's share of realtime legs falls. The German capture
      lost its realtime on 20 October 2026, and nothing reported it, as
      [its caveat](completed-slices.md#caveat-realtime-stops-partway-through-the-capture) sets out.
- [ ] If the warning fires during a capture, investigate the cause before the capture ends. Compare
      the realtime feed's trip ids with the imported timetable's.

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
- 2026-10-19: silver `train_session` hits the same panic, since its `bbox` struct comes before
  its geometry. A filter on `session_id` panics at `spatial_filter.rs:526`.
- 2026-10-24: silver's writers now put the geometry columns first, and `Query::register_silver`
  reads every silver dataset through SedonaDB. Only bronze still meets the panic, since its
  Overture `segment` files keep the release's column order.

## Slice: Rationalise Medallion Types

### Target

Make every reader of the store see each column as the type its row type declares.

Silver's files say what its two geometry columns hold. The column named `geometry` holds the
geography, in lat/lon, and `geometry_projected` holds the geometry, in a country's metres. The code
measures the first on the round Earth and the second on a plane, but a file declares both as planar.
[2026-11-02-geography-and-geometry-columns.md](2026-11-02-geography-and-geometry-columns.md) holds
the assessment.

DuckDB reads each dataset through a view that medallion generates from the dataset's row type. The
view converts what DuckDB's Parquet reader drops, such as a duration, which it reads as an integer
of milliseconds. [2026-11-15-duckdb-types.md](2026-11-15-duckdb-types.md) holds the assessment and
the alternatives.

### Tasks

- [ ] Check each reader of the lat/lon column against a file with spherical edges. SedonaDB treats
      such a column as a separate type, with its own set of functions.
- [ ] If the readers allow it, declare the lat/lon column's edges as spherical. Compare every
      derived dataset before and after.
- [ ] Rename the columns to `geography` and `geometry`, as new versions of the six datasets that
      hold them.
- [ ] Measure a dictionary over the WKB, then GeoArrow-native encoding once SedonaDB reads it.
- [ ] Answer the DuckDB doc's open questions: a CRS on `GEOMETRY`, partition pruning through a
      view, and where the views live.
- [ ] Generate a DuckDB view per dataset from its row type, converting each duration with
      `to_milliseconds`, and each geometry as the first answer allows.
- [ ] Read the views in `/choose-predictor`'s `runs.sql`, in place of its own conversions.

## Slice: Demonstrable Behaviour

### Target

Some of the algorithms / processing we are doing here is getting quite complex, and I am likely to not remember / understand after some time, particularly if I go away and work on something else. So, we should produce a small "book", expressed in notebooks, and some associated blogposts on how this works, following the principles of [our research](2026-10-18-demonstrable-behaviour.md).

Some of the key areas to cover:
* deriving water crossings
* [session crossings and matching](../crates/session_crossings/README.md)
* [predictor](../crates/predictor/README.md)
* evaluation
  * The measure: a notification earns credit by its lead time to the next pass of its crossing,
    with a ramp past N rather than a step. Repeats, false alarms, and misses score 0.
  * The tracker's states, and how a prediction's confidence maps to them.
  * The online and offline pass detectors, and the skew between them: seen, unmatched, and offset.
  * The oracle, which checks the skeleton by scoring perfectly except where passes of a crossing
    fall close together.
  * Intervals from a bootstrap over sessions.
  * The tune/test split by A5 cell:
    * Why a cell rather than a line: `route_name` names a train.
    * Why A5 rather than H3, and how a salt and buckets assign each cell a split.
    * The resolution chosen, and why no resolution steadies the heading angle's p99 between the
      splits.
    * The invariants of the split, and the three plausible errors that only an invariant caught.
  * Scoring each unit by its crossing's cell:
    * A run replays each session whole. Cutting sessions at a cell's edge starts some mid-approach,
      so more passes fall under N after a start, and CrowFlies' precision drops from 0.328 to
      0.299.
    * Each notification, pass, repeat, false alarm, and miss takes the split of its crossing's
      cell. A run without a split scores every unit.
    * False alarms per hour divide by the hours in the split's cells. Each step between two
      samples counts towards the split of its first sample's cell.
    * The skew counts only passes and visits whose crossing lies in the split's cells, so every
      measure a run reports covers one split.
  * Tuning reads only windows that lie wholly in tune cells.
  * LightCone, the predictor the apps run:
    * CrowFlies with a cone around the heading on top, which only lowers a confidence, so it
      alerts on a subset of what CrowFlies alerts on.
    * The heading and speed from the train's last move beyond a radius of stillness, so a
      receiver wandering at rest gives neither.
    * How far trains turn within N, measured over windows, bounds the sweep of the cone's angle.
      The extremes differ between the splits, since a few places hold most reversals.
  * F0.5: why the measure weighs precision above recall, and how β ranks the runs.
  * Choosing settings: a sweep over the tune split, then each predictor's chosen settings run once
    over the test split, and the apps' predictor chosen from those runs alone.
  * How a run is stored:
    * A crossing's transitions within one stay in reach become one episode, holding its path of
      states and the gaps between them.
    * Outcomes drop what other columns give, and sessions sort in route order, so similar rows
      share a page.
    * A migration refuses a run whose new rows do not rebuild its old ones exactly.

### Starting points

- **The evaluation's exploration notebooks.** `notebooks/evaluation/explore1.py` to `explore9.py`
  each answered one question while the evaluation was built, and several explain a step as they go.
  `explore8.py`, for example, sets out what each of a run's choices guards against. Fold them into
  one or more notebooks that demonstrate the evaluation's behaviours or steps, then remove them,
  with their answers in the removing commit. `explore3.py`, `explore4.py`, `explore8.py`, and
  `explore9.py` read gold datasets that runs no longer write, `evaluation_transition_v1`,
  `evaluation_outcome_v2`, and `evaluation_session_v2` among them. Each needs moving to the
  datasets that replace them before it runs.

# Milestone: V2

## Target

- Runs on Web/M5Plus + also: Android/iOS App + also: CoreS3 M5 device with GNSS base.
- Has a predictor that takes into account reachability of a crossing and not just distance,
  either through observational data, or through route connectivity.

## Slice: an observational predictor

### Target

Build the predictor that
[the observational predictor notes](2026-10-03-observational-predictor.md) describe: a map from
(speed, direction, cell) to the crossings reached, learned from recorded journeys. Measure it
against the predictor the apps run, through the evaluation that LightCone's slice built.

The map holds the journeys it was built from. A test on those journeys measures how well it
recalls them, not how well it predicts.
[Tuning and testing](evaluation.md#tuning-and-testing) sets out how the
evaluation splits sessions for LightCone, whose few settings claim to work on any track. This
slice revisits that doc's choices for a map, which claims to predict later trains over track it
has seen:

- **Split by time.** The test set holds journeys captured after those the map was built from.
  The German capture holds two usable days under one timetable, so this needs a later capture.
- **A third set.** The map learns a distribution in every bucket, and a sweep then chooses its
  resolution, buckets, and back-off. Building the map and choosing those settings each need
  journeys the other has not read.
- **Staleness.** A test set captured weeks after the build measures how fast the map goes stale
  as the network and its services change.

## Slice: Collected Possible Precision/Recall Improvements

### Target

Collect the ideas that could raise a predictor's precision or recall beyond what LightCone's slice
chose. Each is an experiment, measured against the chosen predictor through the evaluation, and
adopted only where its runs beat that predictor.

### Debouncing

A run shows whether debouncing a predictor's predictions improves its precision and recall. The
apps can take a predictor without it, so this experiment can be skipped. Each run starts
`/choose-predictor`, so a debounced predictor can replace the one LightCone's slice chose.

- [ ] Debounce `LightCone`'s predictions and `CrowFlies`', each as a setting of its predictor.
- [ ] Run each predictor with and without its debouncing, over the same sessions and seed, and
      compare precision, recall, and false alarms per hour.

### A trumpet around the heading

LightCone keeps the crossings CrowFlies alerts on that lie within a cone around the heading. The
cone stands for what the train can reach within the horizon, given how far it can turn. It grants
the same turn at every speed. A train turns less the faster it goes, since it cannot take a tight
curve at speed, and the shape of the track limits its turns too. The current speed can therefore
predict the largest turn the train can make. Carried ahead along the heading, that limit draws a
shape narrow near the train and wider further out, like a trumpet.
[The trumpet, as a reachable set](2026-11-16-trumpet-reachable-set.md) names the fields that
study this shape, and derives a bound on it from the physics of a train in a curve.

- [ ] Learn the relationship between speed and the turning angle measured in the recorded sessions.
- [ ] Build the trumpet from that relationship, as a filter on CrowFlies' alerts in place of the
      cone.
- [ ] Run it over the sessions LightCone's runs used, and compare it with the chosen predictor.

## Slice: replay the trains whose legs overlap

### Target

Derive train sessions for the trips the ingest drops as `overlapping`. On the capture to 19
October, that was 7,177 of 47,149 trips, about 15%. In most of them, the next leg was captured
after the leg before it, and departs before that leg's older arrival. Trusting the newer capture
where two legs overlap would keep those trips. `motis_ingest` logs each reason it drops a trip, so
the counts show what a change gains.

Also order a trip's legs that share a departure and an arrival, so that unchanged bronze gives the
same train sessions. `docs/architecture.md` claims this of every derivation, and these legs break
the claim. `Legs::new` sorts by departure and arrival, so such legs keep the order the query
returned them in, and `captures` orders only by departure. They pass the overlap check, since
neither departs before the other arrives.

On 31 October, deriving from `motis_segment_v3` rather than `motis_segment_v2` changed 34 of
165,606 sessions, from identical train segments. Each of the 34 trips has legs sharing a departure.
1,566 samples moved, by up to about 68 km, a distance that suggests a wrong order rather than two
valid ones. A stable tie-break alone would then repeat the wrong route. The order may need to follow
the stops, each leg starting where the last one ended.

## Slice: derive the water crossings in Rust

### Target

Derive silver `water_crossing_v2` in Rust, in place of the `v10` notebook, so that unchanged
extracts give the same crossings. `docs/architecture.md` claims this of every derivation, and the
notebook breaks the claim. The slice "Sessionisation and Water crossings per session + Refactor to
Medallion" kept the pipeline a notebook, with only its write in Rust.

A run on 20 October, over the same pinned extracts, changed 42 of the 10,254 crossings that the
12 October run wrote. At least 40 of them moved onto another segment of the same track, a median
of about 5 m away and at most 13.6 m. The gold pack in use comes from a third run, on 11 October.
It differs from the 12 October run by 40 crossings, and from the 20 October run by 48. A moved
crossing takes a new id and compact id, so packing after any run changes what the device carries.

The notebook names a crossing merged from several parts after the part with the longest overlap,
through `idxmax`. A point crossing, where the rail meets water mapped as a line, overlaps nothing,
and the notebook records its overlap as 0 m. A merged point crossing's parts therefore all tie
at 0 m. `idxmax` then takes the first part, and nothing fixes the order of the parts between runs.
Taking another part puts the crossing elsewhere on the same track, within the 100 m merge
distance. That fits how at least 40 of those 42 moved. On 24 October, 642 of the 1,478 merged
crossings were point crossings. That day's run renamed one of them, and two passes in
`silver/session_crossing` then named an id the store no longer held. A moved line crossing needs
another cause.

The notebook also keeps crossings outside the country it derives them for. DE's crossings include
one of the Moselle near Thionville, in France. The evaluation leaves such crossings out of a run
with a split, as a stopgap. Deriving the crossings in Rust cuts them to the country's areas, and
the evaluation then reads silver as it stands. The cut keeps a crossing on a river that forms the
border, such as the Wurm at Herzogenrath, which German trains pass. Its centre can lie just outside
the country's outline.

Consider tracing each dataset back to what it was derived from, such as the Overture release behind
the crossings. A disparity between the newest Overture extract and a silver dataset could then be
told apart from the two reading different releases, which would guide what to do and where to
debug. The evaluation's split checks, for example, read a country's newest extract, while the
crossings they check came from whichever extract built them.

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. That allows a recommendation about which direction to look from the train seat.

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

## Slice: Adding POIs from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POIs by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to identify exactly what the POI is based on the image.

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
