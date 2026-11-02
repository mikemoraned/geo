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

We should implement an evaluation framework which uses advice from apps/lookout/docs/2026-08-01-evaluation.md and applies it to saved sessions from myself (silver/session_v3 table) and from motis (bronze/motis_segment_v3). The idea is to use real recorded data from being on a train or from reported positions of trains to drive an evaluation of what the predictor says about future water crossings compared to when they actually happened. We can use silver/session_crossing_v3 for this, and we may want to apply the same pattern to motis data i.e. treat motis train tracking as a session.

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

Imprecise samples can split one pass into two. Session `71b9c047`, on 26 July, reports about
1,450 m of accuracy, and passes four crossings twice each, 22 s apart. The rule that splits a
session's samples into visits cannot tell such jitter from a real revisit. A real revisit needs
the train to leave and return, which takes a span of space and time. Where a session's
uncertainty exceeds that span, two visits are no longer discernible.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

The evaluation scores predictions against the ground truth the slice "Motis as ground truth"
establishes. Most of that capture has no realtime, as
[its caveat](completed-slices.md#caveat-realtime-stops-partway-through-the-capture) sets out. If
that proves an issue, limit the evaluation data to legs with realtime.

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
- **A Motis train is a train session in silver.** `train_session_sample_v2` holds its position every
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
- [ ] Add an `evaluation` crate, with a binary replaying `silver/session_sample_v3` through a
      predictor and the state machine.
- [ ] Score each pass in `silver/session_crossing_v3`, and each unmatched alert, as `EvaluationRow`
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
