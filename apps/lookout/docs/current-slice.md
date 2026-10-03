# Current Slice: Evaluation framework with associated Crow Flies predictor improvement

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

We should get new motis data by polling motis live in a particular bbox and watching when trains arrive. The idea is that we need to get enough data that we can put together a reasonable size test dataset, and *also* that we gather enough data to do more ambitious stuff with it later, where we use the motis data as effectively input data for a model or a dataset.

If we use transitious.org then we can benefit from more accurate paths (see pfaedle slice), but we should be good citizens and not spam it constantly. We also *probably* don't need this level of route accuracy for a first-cut of a prediction framework. However, it means we should be sure to explicitly model the source of our motis data with metadata about e.g. as a small table in bronze which records which motis version/setup was used and which the actual motis samples in bronze can have a foreign key to. This allows us to ignore sources as needed when building the canonical datasets in Silver.

### What a prediction is

A predictor reports, for each crossing in range, the distance to it, whether it is reachable, and
the instant it is reached. A prediction is the stronger claim: the train passes this crossing within the next N
minutes. It is the state PredictedPass, and entering it raises an alert. The alert is what the
user sees as a notification, and what the evaluation scores.

A crossing with an estimated instant is not yet a prediction. CandidatePass holds a crossing near
enough to consider, with or without an estimate. OutOfReach holds every other crossing, including
one nearby that the predictor rules out. ActualPass holds a crossing the train has passed, until the train
is far enough away that a new alert for it means a second pass.

The evaluation scores the first alert before each pass. A session can pass one crossing more than
once: a train reversing out of a terminus such as Leipzig re-crosses the water it crossed on the
way in. Each pass is scored on its own, so the unit is a (session, crossing, pass) triple, not the
(session, crossing) pair of [2026-08-01-evaluation.md](2026-08-01-evaluation.md).

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

- **An upgrade phase comes first: geo, then crux.** The slice changes the predictor, which uses
  geo, and the crux core, which will send notifications. Upgrading first keeps each upgrade's
  breakage apart from the slice's own work, as the upgrades rule asks.
- **geo moves to 0.33.1, ahead of SedonaDB.** sedona-geo at 0.4.1 resolves geo 0.31, so the host
  tree carries both versions until the SedonaDB 0.5.0 slice. The device tree holds no SedonaDB,
  and carries 0.33.1 alone. Waiting for that slice keeps one geo version, but holds the predictor
  on the older geo through this slice.
- **crux_core moves to 0.20.0, the latest.** The reboot loop of 2026-09-21 struck on 0.16.2 as
  well, so the pin does not prevent reboots. The 0.20 fault in `docs/device.md` was seen on the
  1.90.0 ESP toolchain, and the device has not run 0.20 on esp-1.98.1.0. Newer crux brings the
  framework features that notifications want.
- **A soak before and after the crux move measures its effect on stability.** The baseline soak
  runs after the geo move, so the two soaks differ by crux alone. The soak of 2026-10-03 predates
  the geo move, so it does not serve as the baseline. Each soak runs 30 minutes, the length of the
  clean soaks on record, under the same conditions. Each records reboots, unused stack, free heap,
  and the slowest sentence's time, as the soak of 2026-10-03 did.
- **The state machine lives in `predictor`, over any `Predict`.** The device, the browser, the
  rerun replay, and the evaluation then run the same transitions. A reachable crossing is a
  CandidatePass. It enters PredictedPass when its estimated instant is less than N minutes away. In
  `platform-core` instead, the evaluation and the replay would need the crux core to see a
  transition.
- **The light cone decides reachability, and the current velocity decides the prediction.** From
  the current position and speed, a train's maximum speed and maximum acceleration bound where it
  can be at each instant within the horizon. A crossing outside that bound is unreachable. For a
  reachable crossing, projecting the current velocity forward estimates the instant it is passed.
  The maximum speed and acceleration are recorded per country, each with its source.
- **`LightCone` is a second `Predict`, beside `CrowFlies`.** `CrowFlies` marks every crossing in its
  radius reachable, and stays as the baseline the evaluation compares against. The state machine
  treats the two alike.
- **Each transition debounces with hysteresis and a minimum dwell, the approach geofencing APIs
  take.** A state is entered at one threshold and left at a wider one, and a change holds only once
  it has lasted a minimum time. ActualPass is entered when the closest approach falls within the
  match radius of `match_crossings`, 250 m, and left beyond a larger radius. A survey of common
  approaches comes first, and the values come from replaying sessions.
- **Speed and direction come from the fixes alone.** The light cone ignores the speed and heading
  a phone or receiver reports, so each platform runs the same estimate. The estimate starts as an
  average over the last few fixes. A filter replaces it only if the evaluation shows the average
  limits the result. Reported speed stays in use in `CrowFlies`, which is the constant-velocity
  baseline the evaluation doc asks for.
- **A Rust binary runs the evaluation, and a notebook reads it.** The binary replays each session
  through a predictor and the state machine. It writes one gold row per pass or false alarm,
  with the outcome, the lead time, and the run's parameters, under evaluation results. A marimo
  notebook computes the measures from those rows. This follows "Rust derives; Python reads" in
  `docs/architecture.md`. A notebook driving the `lookout_predictor` binding iterates faster, but
  writes nothing to gold.
- **Ground truth records each pass, not each crossing.** `match_crossings` keeps one pass per
  (session, crossing) today, at the nearest fix. It instead splits the fixes within the match
  radius into visits, separated by a fix outside it, and records a pass per visit. An alert is
  scored against the next pass of its crossing. An alert with no pass following it within the
  horizon is a false alarm.
- **The crossing instant is interpolated along the session path.** `match_crossings` takes the
  instant of the nearest fix today. At line speed, that fix lies over 100 m from the crossing,
  several seconds off. The evaluation doc requires the interpolated instant.
- **All the Motis work is in this slice: the source, the capture, and the evaluation.** V1 asks
  for measurable predictability on Motis data, so the evaluation is incomplete without it.
- **Every Motis capture names its source, in a bronze table.** A source row records the server, its
  feed, its Motis version, and the area it captures. Each captured row carries the source's id, so
  a derivation can leave a source out.
- **Captures with a source id go to a new bronze dataset, `motis_segment_v2`.** `motis_segment`
  stays as written: bronze holds versions, not edits. `motis_ingest` reads both, and treats every
  `motis_segment` row as captured from the local DELFI server. Every derivation reading Motis
  captures reads both datasets.
- **The capture queries the local DELFI server only.** It covers DE, with no usage policy to
  respect. Transitous adds GB and better rail shapes, under a policy that asks for light traffic and
  contact before routine use. With the source table, Transitous later becomes a new source row.
- **`motis_poll` gains an area mode beside its near-GPS mode.** The near-GPS mode captures only where
  the user is travelling. The area mode captures every train in a country, optionally narrowed to
  one region, such as DE and Thuringia. It queries the area's bounding box, from the region's
  Overture `division_area`, and keeps the legs that touch the area.
- **A first capture runs for an hour, then for a day.** A day covers a full timetable's variation,
  and a week is the aim once the day's capture is checked.
- **A Motis train is evaluated as a synthetic session, on chord geometry.** The binary interpolates
  positions along the train's legs in `train_segment`, and matches crossings against them as it
  does a recorded session. DELFI rail legs are straight chords of four points or fewer, so the
  predictor and the truth share the chord. The score therefore measures the predictor on
  straight-line motion. Motis results are always reported apart from recorded sessions. Snapping
  each leg to Overture rail gives real geometry, but that is map-matching, close to the parked
  pfaedle slice.
- **A synthetic session holds a position every 10 s.** The recorded sessions have a median of 10.0 s
  between fixes, so a Motis score and a recorded score share an input rate.
- **The synthetic sessions are built inside the evaluation run, not stored.** They derive
  deterministically from `train_segment`, so a silver dataset of them adds a write and no
  information.
- **The core raises a notification as a crux effect.** On the web, the element shows a banner in
  the page. While the page is hidden, it also raises a system notification through the browser's
  Notification API, which needs permission and a secure context. On the M5, the panel shows a
  banner.

#### Consequences elsewhere

- `docs/device.md` heads a section "`crux_core` after 0.16.2 reboots the device", and tells a
  reader to pin `=0.16.2`. The comment over the pin in `crates/platform/web/core/Cargo.toml` says
  0.20 reboots the board. Both change with the second soak.
- The workspace `Cargo.toml` comment pins geo to the version sedona-geo resolves.
- The SedonaDB 0.5.0 slice in `docs/next-slices.md` moves geo to 0.33 itself, and that task
  becomes moot.
- `App` in `crates/platform-core/src/app.rs` declares `type Capabilities = ()` and takes a `_caps`
  argument in `update`. crux removed both after 0.16.2.
- `silver/session_crossing` can hold several rows for one (session, crossing), and `crossed_at`
  changes meaning. `pack_sessions` reads both. The
  packed sessions change, so `sessions.version` moves.
- `motis_ingest` reads two bronze datasets where it read one.
- `Prediction` in `crates/predictor/src/predict.rs` gains a reachability field, and `CrowFlies`
  sets it.
- The `lookout_predictor` binding in `crates/platform/rerun-py` exposes `CrowFlies` alone.
- The picture in `docs/web.md` shows every crossing in range alike. With states, it shows which
  crossing is predicted.
- The derivation graph in `docs/architecture.md` gains the evaluation run and the Motis source.

#### Open questions

- Whether crux 0.20 changes more of `App` and the shells than `Capabilities` and `caps`. Three
  minor releases separate 0.16.2 from 0.20.0. The first build after the move answers it.
- What follows if crux 0.20 reboots the device. A revert to 0.16.2 keeps the slice moving.
  `docs/device.md` also lists bisecting 0.17 to 0.19, at one flash and a 15-minute soak each.
- Whether one poll over a whole country fits one `map/trips` request at zoom 8. If it does not,
  the area mode splits the bounding box into tiles.

#### Rejected / deferred

- **A filter over the fixes, such as a Kalman filter.** Deferred until the evaluation shows the
  average limits the result. It then comes from a crate that builds for the device as well as the
  host.
- **Transitous as a source.** Deferred until a capture needs GB or curved rail. It becomes a new
  source row.
- **Snapping Motis legs to Overture rail.** Deferred to the parked pfaedle slice.

### Tasks

#### Upgrades

Each upgrade lands in a commit of its own, before the slice's own work starts.

- [ ] Move `geo` to 0.33.1 in the workspace `Cargo.toml`, `crates/domain`, and
      `crates/predictor`. Rewrite the workspace comment on its pin.
- [ ] Run `just test-no-docker` and `just test-geo`, and build with `just m5plus-build-release`.
- [ ] Flash the release build and soak it for 30 minutes, as the baseline. The user runs this.
- [ ] Move `crux_core` to `=0.20.0` in `platform-core`, `platform/web/core`, `platform/web/bridge`,
      `platform/m5/core`, and `platform/m5/m5plus`.
- [ ] Drop `type Capabilities` and the `_caps` argument from `App` in
      `crates/platform-core/src/app.rs`.
- [ ] Run `just test-no-docker`, and build with `just wasm` and `just m5plus-build-release`.
- [ ] Flash the release build and soak it for 30 minutes under the baseline's conditions. The
      user runs this.
- [ ] Rewrite the crux section of `docs/device.md` from the two soaks, and the comment over the
      pin in `crates/platform/web/core/Cargo.toml`.
- [ ] Mark the geo task moot in the SedonaDB 0.5.0 slice in `docs/next-slices.md`.

#### Motis capture

First after the upgrades, so the capture accumulates while the rest is built.

- [ ] Measure one `map/trips` poll over the DE bounding box at zoom 8: its response size, its legs,
      and its duration.
- [ ] Add `MotisSourceRow` to `crates/medallion-model/src/motis.rs`: source id, base URL, feed,
      Motis version, country, optional region, and the instant it was registered.
- [ ] Add the bronze dataset `motis_segment_v2`, holding `MotisSegmentRow` with `source_id`. Leave
      `motis_segment` as written.
- [ ] Register the source in `motis_poll` at startup, writing its row only when absent.
- [ ] Read both datasets in `motis_ingest`, mapping `motis_segment` rows to the local DELFI source,
      and carry `source_id` into `TrainSegmentRow`.
- [ ] Add the area mode to `motis_poll`: a country and an optional region, queried by the area's
      bounding box, keeping the legs that touch the area.
- [ ] Add a recipe for the area mode beside `bronze-poll-motis`.
- [ ] Capture DE and Thuringia for an hour, then for a day. The user runs it. A week's capture
      follows once the day's is checked.
- [ ] Describe `motis_segment_v2` and the source table in `docs/medallion.md`, and the area mode in
      `docs/motis.md`.

#### Ground truth

- [ ] Split each session's fixes within the match radius into visits, and record a pass per
      visit, in `crates/session_crossings/src/matching.rs`. Test it on a session that reverses
      over the same crossing.
- [ ] Interpolate the crossing instant between the two fixes bracketing the closest approach, in
      `crates/session_crossings/src/matching.rs`.
- [ ] Derive `silver/session_crossing` with `just silver-session-crossings`, and record the passes
      per country.
- [ ] Re-run `just gold-pack-sessions`, and move `sessions.version` to the new version.

#### The state machine

- [ ] Survey how geofencing APIs and similar systems debounce state changes, and record what the
      state machine takes under Decisions.
- [ ] Add reachability to `Prediction` in `crates/predictor/src/predict.rs`, and set it in
      `CrowFlies` for every crossing in its radius.
- [ ] Add the four states and their transitions to `predictor`, over any `Predict`, with N as a
      parameter.
- [ ] Report each transition as an event, entry into PredictedPass being an alert.
- [ ] Debounce each transition with hysteresis and a minimum dwell, with the thresholds as
      parameters. Their values wait on the evaluation.

#### The evaluation, on recorded sessions

- [ ] Add an `evaluation` crate, with a binary replaying each session in `silver/session_sample`
      through `CrowFlies` and the state machine.
- [ ] Classify each pass in `silver/session_crossing` as hit, late, early, or miss, by the first
      alert before it, with its lead time. Classify an alert with no following pass as a false
      alarm.
- [ ] Add `EvaluationRow` to `crates/medallion-model`, and write it under evaluation results, with
      the predictor, its parameters, and the input versions as columns.
- [ ] Add a recipe for the run.
- [ ] Add a marimo notebook in `notebooks/` reading the results. It reports the useful alert rate,
      the false alarm ratio, false alarms per hour, and the critical success index, each with n.
- [ ] Add the threshold sweep and the horizon diagnostic to the notebook.
- [ ] Run `CrowFlies` across a sweep of N, and record its results as the baseline.

#### The light cone

- [ ] Record a maximum speed and a maximum acceleration for trains in DE and in GB, each with its
      source.
- [ ] Add a velocity estimate to `predictor`, averaged over the last few fixes, from positions
      alone.
- [ ] Add `LightCone` beside `CrowFlies`, implementing `Predict`. The cone marks each crossing
      reachable or not, and the projected velocity estimates the instant for a reachable one.
- [ ] Run it through the evaluation across a sweep of N, and compare it with the baseline.
- [ ] Choose the operating point from the sweep, and record the evidence for it.
- [ ] Bind `LightCone` and the state machine in `crates/platform/rerun-py`, and draw each
      crossing's state in the replay. Low priority: do it when a replay explains a result.

#### The evaluation, on Motis trains

- [ ] Build a synthetic session from each train's legs in `silver/train_segment`, interpolating a
      position every 10 s along each leg.
- [ ] Match crossings against the synthetic sessions with `session_crossings::matching::passes`.
- [ ] Score the synthetic sessions in the same run, with the source on each row.
- [ ] Report the Motis results in the notebook apart from the recorded sessions.

#### In the core and the shells

- [ ] Replace `CrowFlies` with `LightCone` and the state machine in
      `crates/platform-core/src/app.rs`, at the chosen operating point.
- [ ] Add a notification effect to `Effect`, raised on entry into PredictedPass.
- [ ] Show the banner in the web element, and raise a system notification while the page is
      hidden. Ask for the permission from the page.
- [ ] Show the banner on the M5 panel, in `crates/platform/m5/core/src/panel.rs`.
- [ ] Draw each crossing's state in the web picture and on the M5 panel.
- [ ] Describe the notification and the states in `docs/web.md` and `docs/device.md`.
- [ ] Build with `just wasm` and `just m5plus-build-release`, and flash the device. The user rides
      with it and the live page.

#### Wrap-up

- [ ] Add the evaluation run and the Motis source to the derivation graph in
      `docs/architecture.md`.
- [ ] Delete `docs/2026-08-01-evaluation.md` at the end of this slice. It is a dated
      assessment of how to measure a predictor, written before one existed and before there
      was any ground truth to measure against, and kept for the history of the decision.
      Anything in it still holding by then belongs in the tasks above, in the notebooks
      that implement the measures, or in a durable doc alongside `medallion.md`; the rest —
      the rejected alternatives, the reasoning about metrics from other domains — goes
      stale once a first run has actually produced numbers.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test` for the Docker
      tests and the notebook recipes.
