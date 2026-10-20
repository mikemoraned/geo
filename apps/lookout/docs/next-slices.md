# Next Slices

# Milestone: V1 MVP

## Target

This has a working but minimal prediction framework that has measurable predictability based on
observed data from motis. It runs on Web and on the M5Plus. It can give notifications visually on
Web/M5Plus (in a way suitable to platform capabilities).

It is ok if the predictor is a simple variation on a Crow Flies predictor that takes into account
your speed and distance from crossings.

It need only support UK and Germany.

## Slice: Evaluation framework with associated Crow Flies predictor improvement

### Target

I have some larger ideas for how to come up with a good predictor, but for now I want to prove I can improve things + measure that improvement in some way.

So, I'd like to go from a very simple Crow Flies predictor I have now, which effectively just shows distance to nearby crossings, to one that takes into account current speed and average direction, and predicts when a crossing will be passed in next N minutes.

### Straw Man

This approach means we have to become a lot crisper in what a prediction means. Right now the display just shows a radar style view which isn't really making a prediction and is instead just a map of nearby crossings. My thinking here is that, for each crossing, we need to have a little state-machine, something like:

States:
* OutOfReach: this is maybe an implicit state that any crossing sits in, if it is just in the pointset data, and isn't reachable at all
* CandidatePass: this is when it becomes nearby enough to be considered as a possible crossing
* PredictedPass: this is when we expect we will actually pass by this crossing in the next N minutes
* ActualPass: we just actually passed the crossing

Transitions:
* OutOfReach <-> CandidatePass: a crossing can oscillate in and out of being a candidate
* OutOfReach -> PredictedPass or CandidatePass -> PredictedPass: this is us making a prediction, and this transition should lead to a notification of some kind to the user
* PredictedPass -> CandidatePass or PredictedPass -> OutOfReach: this counts as a failed prediction
* PredictedPass -> ActualPass: a successful prediction. Note that a crossing stays in the ActualPass state until we've gotten some distance away from it to avoid re-notifying repeatedly. In general, the state transitions should probably have some sort of debouncing to avoid frequent flips in and out of states.
* ActualPass -> PredictedPass: it's always possible we can pass somewhere multiple times, so this is a new prediction
* ActualPass -> CandidatePass: still within reach but not currently predicted
* ActualPass -> OutOfReach: it's now distance from us and we don't even think it is a candidate

In this framework, depending on how clever the predictor is, we may be able to display a crossing but still label it as OutOfReach. This could be, for example, if we decide it is technically close by but we are not going to reach it because it's not on our current track.

However, the cleverness of the predictor we do in this slice should be limited to using a current-speed/direction + light-cone approach i.e. predict what we will reach based on where we will pass by based on current vector. This can start with a low-level of cleverness about current speed/direction (i.e. just literal current speed) and then start building up an overall speed/direction based on an average over last few fixes and/or on speed reported by the GPS device. 

I suspect on Web we could benefit from the cleverness of the GPS baked into the phone, as it's probably doing something clever, but I'd prefer not to use that and instead do it ourselves, as then we have consistent performance across platforms. I am thinking here of some sort of Kalman-filter or similar.

We should implement an evaluation framework which uses advice from apps/lookout/docs/2026-08-01-evaluation.md and applies it to saved sessions from myself (silver/session table) and from motis (bronze/motis_segment). The idea is to use real recorded data from being on a train or from reported positions of trains to drive an evaluation of what the predictor says about future water crossings compared to when they actually happened. We can use silver/session_crossing for this, and we may want to apply the same pattern to motis data i.e. treat motis train tracking as a session.

### What a prediction is

Each crossing is in one of four states:

- **OutOfReach**: the predictor rules it out.
- **CandidatePass**: reachable, but not expected within N minutes.
- **PredictedPass**: expected within N minutes. Entering it raises an alert, which is the
  notification the user sees and what the evaluation scores.
- **ActualPass**: recently passed, until the train is far enough away that a new alert means a
  second pass.

The evaluation scores the first alert before each pass. A train reversing out of Leipzig passes
the same crossings twice, so the unit is (session, crossing, pass), not the (session, crossing)
pair of [2026-08-01-evaluation.md](2026-08-01-evaluation.md).

Imprecise samples can split one pass into two. Session `43fffff4`, on 26 July, reports about
1,450 m of accuracy, and passes four crossings twice each, 22 s apart. The rule that splits a
session's samples into visits cannot tell such jitter from a real revisit. A real revisit needs
the train to leave and return, which takes a span of space and time. Where a session's
uncertainty exceeds that span, two visits are no longer discernible.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

The evaluation scores predictions against the ground truth the slice "Motis as ground truth"
establishes.

#### Prediction

- **The state machine lives in `predictor`, over any `Predict`**, so the device, the browser, the
  replay, and the evaluation share it.
- **A light cone decides reachability, and the current velocity decides the prediction.** A
  train's maximum speed and acceleration bound where it can be within the horizon. Projecting the
  current velocity estimates when a reachable crossing is passed. The bounds are recorded per
  country, with sources.
- **`LightCone` sits beside `CrowFlies`.** `CrowFlies` stays as the baseline, and marks every
  crossing in its radius reachable.
- **Speed and direction come from the fixes alone**, averaged over the last few, so every platform
  estimates alike. `CrowFlies` keeps the reported speed, as the constant-velocity baseline.
- **Transitions debounce with hysteresis and a minimum dwell**, as geofencing APIs do. ActualPass
  is entered within the 250 m match radius, and left beyond a larger one.

#### Evaluation

- **A Rust binary scores, and a notebook measures.** The binary writes a gold row per pass or false
  alarm, with the outcome, the lead time, and the run's parameters. An alert with no pass following
  within the horizon is a false alarm.
- **A Motis train is a train session in silver.** `train_session_sample` holds its position every
  10 s, the recorded sessions' median, along its legs. Motis results are reported apart from
  recorded sessions.

#### Notifications

- **The core raises a notification as a crux effect, on entry into PredictedPass.** The web
  element shows a banner, and a system notification while the page is hidden. The M5 panel shows a
  banner.

#### Consequences elsewhere

- `Prediction` gains a reachability field.
- The `lookout_predictor` binding in `crates/platform/rerun-py` exposes `CrowFlies` alone.
- The picture in `docs/web.md` and the derivation graph in `docs/architecture.md` change.

#### Open questions

- Why the sentence that scans takes 262 ms on the device rather than 8.5 ms. geo 0.32 takes it
  to 119 ms, and crux 0.20 to 262 ms. On the host, neither changes it, so the cause lies in the
  device build. The light cone adds work to that sentence, so the answer matters before it lands.

#### Rejected / deferred

- **A Kalman filter**, until the evaluation shows averaging limits the result. It then comes from
  a crate that builds for the device.

### Tasks

#### The state machine

- [ ] Survey how geofencing APIs debounce, and record the choice under Decisions.
- [ ] Add reachability to `Prediction`, and set it in `CrowFlies`.
- [ ] Add the four states to `predictor`, reporting each transition as an event.
- [ ] Debounce each transition, with its thresholds as parameters.

#### The evaluation

- [ ] Research how to represent a crossing's discernibility in space and time, before scoring a
      second pass. Compare how closely a real train can revisit a crossing with how uncertain a
      session's samples leave the train. Look for prior art, such as scale space in images.
      Record the findings under Decisions.
- [ ] Add an `evaluation` crate, with a binary replaying `silver/session_sample` through a
      predictor and the state machine.
- [ ] Score each pass in `silver/session_crossing`, and each unmatched alert, as `EvaluationRow`
      under gold evaluation results.
- [ ] Add a recipe for the run.
- [ ] Add a marimo notebook: the headline measures with n, the threshold sweep, and the horizon
      diagnostic.
- [ ] Run `CrowFlies` across a sweep of N, as the baseline.
- [ ] Score the train sessions, and report them apart in the notebook.

#### The light cone

- [ ] Record a maximum speed and acceleration for trains in DE and GB, each with its source.
- [ ] Add a velocity estimate to `predictor`, averaged over the last few fixes.
- [ ] Add `LightCone` to `predictor`.
- [ ] Evaluate it across a sweep of N against the baseline, and choose the operating point.
- [ ] Bind `LightCone` and the state machine in `crates/platform/rerun-py`. Low priority.

#### Notifications

- [ ] Run `LightCone` and the state machine in `crates/platform-core/src/app.rs`, at the operating
      point.
- [ ] Add a notification effect to `Effect`.
- [ ] Show it in the web element, with a system notification while the page is hidden, and on the
      M5 panel.
- [ ] Draw each crossing's state in the web picture and on the M5 panel.
- [ ] Describe the notification and the states in `docs/web.md` and `docs/device.md`.
- [ ] Build both shells, and flash the device. The user rides with it.

#### Wrap-up

- [ ] Add the evaluation to the derivation graph in `docs/architecture.md`.
- [ ] Fold what holds of `docs/2026-08-01-evaluation.md` into a durable doc, and delete it.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.

# Milestone: V2

## Target

- Runs on Web/M5Plus + also: Android/iOS App + also: CoreS3 M5 device with GNSS base.
- Has a predictor that takes into account reachability of a crossing and not just distance,
  either through observational data, or through route connectivity.

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

## Slice: replay the trains whose legs overlap

### Target

Derive train sessions for the trips the ingest drops as `overlapping`. On the capture to 19
October, that was 7,177 of 47,149 trips, about 15%. In most of them, the next leg was captured
after the leg before it, and departs before that leg's older arrival. Trusting the newer capture
where two legs overlap would keep those trips. `motis_ingest` logs each reason it drops a trip, so
the counts show what a change gains.

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
What makes the runs differ is unknown.

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
- 2026-10-19: silver `train_session` hits the same panic, since its `bbox` struct comes before
  its geometry. A filter on `session_id` panics at `spatial_filter.rs:526`. `Query::register_silver`
  now reads any silver dataset holding a struct or a list as plain parquet, so its geometry arrives
  as WKB. Once a release fixes the panic, that branch goes, and these datasets read their geometry
  through SedonaDB again.

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
