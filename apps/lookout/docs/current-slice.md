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

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

#### Upgrades

- **geo, then crux, before the slice's own work, each in its own commit.** The slice changes
  both.
- **geo 0.33.1, ahead of SedonaDB.** The host tree also carries geo 0.31, for sedona-geo, until
  the SedonaDB 0.5.0 slice. The device tree carries 0.33.1 alone.
- **crux_core 0.20.0.** The reboots of 2026-09-21 struck on 0.16.2 too. The 0.20 fault in
  `docs/device.md` was on the 1.90.0 toolchain, and 0.20 has not run on esp-1.98.1.0.
- **A 30-minute soak either side of the crux move**, both after the geo move and under the same
  conditions. Each records reboots, unused stack, free heap, and the slowest sentence.

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

#### Ground truth and evaluation

- **A pass per visit, at an interpolated instant.** `match_crossings` keeps one pass per
  (session, crossing) today, at the nearest fix, which at line speed lies over 100 m off. It
  instead splits the fixes within the match radius into visits, and interpolates each pass's
  instant between the bracketing fixes.
- **A Rust binary scores, and a notebook measures.** The binary writes a gold row per pass or false
  alarm, with the outcome, the lead time, and the run's parameters. An alert with no pass following
  within the horizon is a false alarm.
- **A Motis train is a synthetic session, built inside the run.** It holds a position every 10 s,
  the recorded sessions' median, along its legs in `train_segment`. The legs are straight chords, so
  the score measures straight-line motion, and Motis results are reported apart.

#### Motis capture

- **A bronze source table** records the server, its feed, its Motis version, and its area. Each
  captured row carries its source's id.
- **New captures go to `motis_segment_v2`, and `motis_segment` stays as written.** A reader of
  Motis captures reads both, and treats `motis_segment` rows as from the local DELFI server.
- **The local DELFI server is the only source.** It covers DE, with no usage policy.
- **`motis_poll` gains an area mode**: a country and an optional region, such as DE and
  Thuringia. It queries the area's bounding box, from Overture's `division_area`, and keeps the
  legs touching the area.
- **A first capture runs an hour, then a day**, and a week once the day's is checked.

#### Notifications

- **The core raises a notification as a crux effect, on entry into PredictedPass.** The web
  element shows a banner, and a system notification while the page is hidden. The M5 panel shows a
  banner.

#### Consequences elsewhere

- `docs/device.md` and the pin comment in `crates/platform/web/core/Cargo.toml` say crux after
  0.16.2 reboots the device.
- The workspace `Cargo.toml` comment ties geo to sedona-geo's version. The SedonaDB 0.5.0 slice's
  geo task becomes moot.
- `App` in `crates/platform-core/src/app.rs` loses `type Capabilities` and the `_caps` argument.
- `Prediction` gains a reachability field.
- `silver/session_crossing` can hold several rows per (session, crossing), so the packed sessions
  and `sessions.version` change.
- The `lookout_predictor` binding in `crates/platform/rerun-py` exposes `CrowFlies` alone.
- The picture in `docs/web.md` and the derivation graph in `docs/architecture.md` change.

#### Open questions

- Whether crux 0.20 changes more than `Capabilities` and `caps`. The first build answers it.
- What follows if 0.20 reboots the device: a revert to 0.16.2, or bisecting 0.17 to 0.19.
- Whether a whole-country poll fits one `map/trips` request. If not, the area mode tiles it.

#### Rejected / deferred

- **A Kalman filter**, until the evaluation shows averaging limits the result. It then comes from
  a crate that builds for the device.
- **Transitous**, until a capture needs GB or curved rail. It becomes a new source row.
- **Snapping Motis legs to rail**, to the parked pfaedle slice.

### Tasks

#### Upgrades

- [ ] Move `geo` to 0.33.1 in the workspace, `crates/domain`, and `crates/predictor`, and rewrite
      its pin comment.
- [ ] Run `just test-no-docker`, `just test-geo`, and `just m5plus-build-release`.
- [ ] Soak the release build for 30 minutes, as the baseline. The user runs this.
- [ ] Move `crux_core` to `=0.20.0` in every crate pinning it, and drop `Capabilities` and `_caps`
      from `App`.
- [ ] Run `just test-no-docker`, `just wasm`, and `just m5plus-build-release`.
- [ ] Soak for 30 minutes under the baseline's conditions. The user runs this.
- [ ] Rewrite the crux section of `docs/device.md` and the `web/core` pin comment from the soaks.
- [ ] Mark the geo task moot in the SedonaDB 0.5.0 slice in `docs/next-slices.md`.

#### Motis capture

Next, so data accumulates while the rest is built.

- [ ] Measure one `map/trips` poll over DE at zoom 8: its size, its legs, and its duration.
- [ ] Add `MotisSourceRow` and the bronze dataset `motis_segment_v2` to `crates/medallion-model`.
- [ ] Register the source in `motis_poll` at startup, writing it only when absent.
- [ ] Read both datasets in `motis_ingest`, and carry `source_id` into `TrainSegmentRow`.
- [ ] Add the area mode to `motis_poll`, and a recipe for it beside `bronze-poll-motis`.
- [ ] Capture DE and Thuringia for an hour, then for a day. The user runs it.
- [ ] Describe the source and `motis_segment_v2` in `docs/medallion.md`, and the area mode in
      `docs/motis.md`.

#### Ground truth

- [ ] Record a pass per visit in `crates/session_crossings/src/matching.rs`. Test it on a session
      that reverses over a crossing.
- [ ] Interpolate each pass's instant between the bracketing fixes.
- [ ] Run `just silver-session-crossings` and `just gold-pack-sessions`, and move
      `sessions.version`.

#### The state machine

- [ ] Survey how geofencing APIs debounce, and record the choice under Decisions.
- [ ] Add reachability to `Prediction`, and set it in `CrowFlies`.
- [ ] Add the four states to `predictor`, reporting each transition as an event.
- [ ] Debounce each transition, with its thresholds as parameters.

#### The evaluation

- [ ] Add an `evaluation` crate, with a binary replaying `silver/session_sample` through a
      predictor and the state machine.
- [ ] Score each pass in `silver/session_crossing`, and each unmatched alert, as `EvaluationRow`
      under gold evaluation results.
- [ ] Add a recipe for the run.
- [ ] Add a marimo notebook: the headline measures with n, the threshold sweep, and the horizon
      diagnostic.
- [ ] Run `CrowFlies` across a sweep of N, as the baseline.
- [ ] Score the Motis trains as synthetic sessions, and report them apart in the notebook. Only
      after the day's capture.

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

- [ ] Add the evaluation and the Motis source to the derivation graph in `docs/architecture.md`.
- [ ] Fold what holds of `docs/2026-08-01-evaluation.md` into a durable doc, and delete it.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.
