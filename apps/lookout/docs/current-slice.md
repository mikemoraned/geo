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

N is how far ahead the predictor looks. Each crossing is in one of four states:

- **OutOfReach**: the predictor rules it out.
- **CandidatePass**: reachable, but not expected within N.
- **PredictedPass**: expected within N. Entering this state raises an alert.
- **ActualPass**: recently passed. It lasts until the train is far enough away that a new alert
  means a second pass.

Three rules decide what counts:

- **Each pass counts once, scored by the first alert before it.** A train reversing out of Leipzig
  passes the same crossings twice, so each pass has its own score.
- **One bridge makes one notification.** A bridge holds several crossings, one per track and water
  feature, so their alerts arrive together. The 283 recorded passes fall at 106 moments.
- **A pass with no time for an alert does not count.** A pass less than N after a session's first
  sample cannot be warned about. At 60 s, this covers 10% of recorded passes and 4% of train session
  passes.

GPS error can split one pass into two. Session `71b9c047`, on 26 July, reports about 1,450 m of
accuracy, and passes four crossings twice each, 22 s apart. A real second pass needs the train to
leave and come back. Where GPS error is bigger than that trip, the two cannot be told apart.

### What the evaluation reads

Measured on 2026-11-03.

| | Recorded sessions | Train sessions |
| --- | --- | --- |
| Where the data comes from | A phone on a train | Motis, along each train's legs |
| Countries | 41 sessions in DE, 11 in GB | DE alone for now |
| Sessions | 52, of which 32 moved | 33,830 with realtime on every leg, of 163,402 captured |
| Samples | 6,364 | 11.0M, of 63.7M |
| Passes | 283, of which 22 in GB | Not derived yet, about 17 per session in a sample |
| Position | Real, with GPS error | Worked out along the track, with no error |
| Speed | Reported on 77% of samples | Not reported, and steady along each leg |
| Time between samples | Usually 10 s, with gaps | Exactly 10 s |

Of the 33,830 train sessions, 33,726 are regional, 60 high-speed, and 44 long-distance. A leg is a
train's journey between two stops.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03, and by
answers on 2026-11-03. What is still unconfirmed sits under
[Proposed, not yet confirmed](#proposed-not-yet-confirmed).

#### The data

- **A Motis train is a train session in silver.** `train_session_sample_v2` holds its position every
  10 s, along its legs. Motis results are reported apart from recorded sessions.
- **Train sessions are fixed before they are scored.**
  [Impossible steps in train sessions](#impossible-steps-in-train-sessions) sets out the problem.
  Leaving the affected sessions out is cheaper, and leaves the derivation wrong for other readers.
- **Only train sessions with realtime on every leg are scored**, from the capture
  `delfi-2026-10-10.route-shapes.within`. Most of that capture has no realtime, as
  [its caveat](completed-slices.md#caveat-realtime-stops-partway-through-the-capture) sets out. The
  first open question returns to this with new evidence.

#### The predictor

- **The state machine lives in `predictor`, on top of any `Predict`**, so the device, the browser,
  the replay, and the evaluation share it.
- **`LightCone` sits beside `CrowFlies`, which stays as the baseline.** `CrowFlies` treats every
  crossing in its radius as reachable, and uses the speed the receiver reports.
- **A light cone decides what is reachable, and speed and direction decide what is predicted.** A
  train's maximum speed and acceleration limit how far it can get within N. The limits are recorded
  per country, with sources.
- **Speed and direction come from the fixes alone**, averaged over the last few, so every platform
  estimates alike.
- **A crossing changes state only after a clear, lasting change.** Leaving a state takes a bigger
  change than entering it, and a new state has to hold for a minimum time. Geofencing APIs work this
  way. ActualPass starts within the 250 m match radius, and ends beyond a larger one.

#### The evaluation

- **A Rust binary scores, and a notebook measures.** The binary writes a gold row per pass or false
  alarm, with the outcome, the warning time, and the run's settings.
- **Alerts merge into one notification per bridge, and scoring stays per crossing.** The notebook
  gives counts for passes and for distinct moments, since one bridge is not several observations.
- **`evaluation-py` shows one session's replay**, as `session-crossings-py` shows one session's
  matching. It returns the fixes, predictions, state changes, and alerts as Arrow, and the notebook
  draws them on a map.

#### GPS realism

- **A model fitted to recorded samples adds GPS error to a train session at replay.** Replaying
  Motis shows the effects of track layout and timing, and nothing of GPS error.
- **This follows the first `LightCone` results, and comes before the final settings.** Settings
  chosen without GPS error look better than they are.
- **Passes still come from the error-free positions.** A noisy train session then has a known truth.
  A recorded session has none, since its passes come from the same imprecise samples.
- **The research into telling a real second pass from jitter runs here**, since noisy train sessions
  give it a known answer. Until then, passes are scored as `session_crossing_v3` stores them. Repeat
  passes are rare: 8 crossings in 2 recorded sessions, and 0.6% of the crossings train sessions
  pass.

#### Notifications

- **The core raises a notification as a crux effect, once per bridge.** The web element and the M5
  panel each show a banner.
- **`/live` holds a screen wake lock, as `/record` does**, so the page stays visible on the train.

#### Proposed, not yet confirmed

The data:

- **A train session's passes are a new silver dataset, `train_session_crossing_v1`.**
  `session_crossing_v3` names a device, and reads recorded sessions alone. The new dataset swaps
  `device_id` for `source_id` and `trip_id`, as `train_session_v2` does for `session_v3`.
- **The time between two legs that do not join stays unsampled.** The train then disappears for that
  time, as in a tunnel, rather than waiting and then jumping. This alone clears an estimated 81% of
  the sessions with an impossible step.

The predictor:

- **A cone around the direction of travel decides what lies ahead, and its width is tried across a
  range.** A crossing passed 60 s later lies a median of 10° off the direction of travel, and 40° at
  the 90th percentile. A 10° cone loses half of those passes, and a 45° cone loses 8%.
- **A fix beyond an accuracy limit moves the clock, and nothing else.** Expected arrivals keep
  counting down through a stretch of poor GPS. The GPS realism runs choose the limit.
- **Crossings within 250 m of one another make one notification.** Crossings passed at one moment
  lie a median of 6 m apart, and 227 m at the 99th percentile, so no new setting is needed.
- **The core uses the higher speed limit of DE and GB**, since a shell knows no country.
- **Debouncing follows the first baseline**, which shows how often a state flips.

The scoring:

- **An alert with no pass in the next 5 minutes is false.** The earlier rule used N, which marks an
  alert false for a train that merely slows down.
- **An alert too near a session's end or a gap is left out**, since nothing shows what happened
  next.
- **False alarms per hour count time in motion.** One recorded session holds two samples 20 h apart,
  43% of all recorded hours. Hours count steps within the 10-minute session gap, at over walking
  pace.
- **Each pass's row holds the arrival estimate's error at 30, 60, 120, and 300 s before the pass.**
  Storing every prediction costs a row per sample and crossing.

The code:

- **Scoring sits apart from the store.** `evaluation` holds the replay, the scoring, and the GPS
  model. `evaluation-runner` holds the `evaluate` binary and its reads. `evaluation-py` then avoids
  the 5 min 50 s rebuild of an extension over the store.
- **A replay runs in `f32`**, as both shells do, and reads silver `water_crossing_v2`, whose ids the
  passes name.
- **Each session's predictor gets only the crossings near its samples.** Scanning all 5,749 German
  crossings for 11.0M samples costs about 16 min of CPU per run. Within 27 km of a session's samples
  lie 198 crossings on average.
- **The search radius grows with N.** The current 5 km gives a train at 300 km/h 60 s of warning.

#### Consequences elsewhere

- `Prediction` gains a reachability field.
- `Predicting::Ready` holds a `CrowFlies`, and `Model` reads five accessors from it that `Predict`
  lacks. `LightCone` needs them too.
- `Model::radius_metres`, `panel::within`, and the outer ring of the web picture all state how far
  the core looks. Under `LightCone`, that distance changes with speed.
- The match on `Effect` in `m5plus.rs` and `dispatch` in `lookout-predictor.js` each gain an arm.
- The `lookout_predictor` binding in `crates/platform/rerun-py` keeps exposing `CrowFlies` alone.
- `docs/medallion.md` names `run_date=<date>/run_id=<id>` for evaluation results, and no code writes
  it. `medallion-py` reads silver and bronze alone.
- `ALL` and `declaration.rs` in `medallion-model` gain the new datasets, and `TARGETS` gains the
  silver one.
- The picture in `docs/web.md` and the derivation graph in `docs/architecture.md` change.

#### Open questions

For the user:

1. **Does the evaluation score every train, or only realtime ones?** Realtime and timetable regional
   trains score alike in [the preview](#a-rough-preview), and [realtime mostly moves a train in
   time](#what-realtime-changes). Realtime-only leaves 104 fast trains, and fast trains give the
   most false notifications. The recommendation is every train, with realtime reported apart.
2. **Is the acceleration limit worth building?** An alert fires once distance divided by speed is
   within N, and any such crossing already lies within the limits' reach. The limits therefore
   change only what shows as OutOfReach. The recommendation is to keep the maximum speed, to reject
   absurd speeds, and to drop acceleration.
3. **Is "what lies ahead" a cone or a lane?** The Straw Man's "where we will pass by" describes a
   lane: how far to the side a crossing lies at closest approach. The preview scores them alike. The
   recommendation is to try both.
4. **Does a reported speed count, where a receiver gives one?** The Straw Man prefers working it
   out, for consistency. Both the phone and the device report speed, and with poor fixes ignored,
   the reported speed beats the averaged one on recorded sessions: 80% against 76%. The
   recommendation is to try both.
5. **What were the seven recorded sessions with poor GPS throughout?** They ran on 19 July at 14:16
   and 16:10, 22 July at 18:44, and 25 July at 08:30. The others ran on 26 July at 10:07, and 31
   July at 09:12 and 13:05. They hold 82% of all poor samples. The answer decides whether the model
   has one kind of session or two. The recommendation is to report "GPS working" and "no GPS" apart.
6. **What balance is good enough?** The final settings need a goal, such as 85% of passes with a
   useful alert and at most 2 false notifications an hour. Recorded sessions reach about 72% at 2.9
   in the preview.
7. **Is a minute still the right warning, with 30 s to 2 min counting as useful?** In the preview, N
   of 60 s gives a median warning of 60 s. N of 120 s drops recorded sessions to about 50% useful.
8. **What happens to the train sessions still jumping after the main fix?** About 628 remain, or
   0.38%. Giving legs that take no time some time leaves 262. The recommendation is to leave the
   rest out and count them.
9. **Does the UK capture widen its poll window?** A late poll or a shifting delay loses legs, as
   [Why legs go missing](#why-legs-go-missing) shows. The recommendation is a window of 10 minutes
   either side, as a task in the slice "Motis as ground truth for the UK".
10. **Does this slice split?** It holds 59 tasks. The recommendation is three slices, each ending in
    something to show: measuring `CrowFlies`, a better predictor with GPS realism, and
    notifications.
11. **What does a banner say, and when does it clear?** The device carries no crossing names. The
    recommendation is "Crossing in 0:45" with the distance, clearing on a pass or a failed
    prediction.
12. **Does the kiosk show banners?** It replays a journey in a minute, so a 60 s warning lasts under
    a second. The recommendation is to leave them out of the kiosk for V1.
13. **Does GB need its own evidence before the final settings?** It has 22 recorded passes and no
    train capture. The recommendation is to choose from DE, and to check in GB once the UK capture
    lands.
14. **Can you record more sessions?** Only 23 recorded sessions hold a pass, and none cover the
    device. Fast trains and GB would help most.

For the work to answer:

- Why handling one GPS update takes 262 ms on the device rather than 8.5 ms. geo 0.32 takes it to
  119 ms, and crux 0.20 to 262 ms. On the host, neither changes it, so the cause lies in the device
  build. The light cone adds work to that update, so the answer matters before it lands.
- Whether two states fit recorded samples. Fixed odds of leaving a state put a stretch's median near
  70% of its mean. Poor stretches have a median of 2 samples and a mean of 11. If the plots
  disagree, stretch lengths come from the recorded distribution.
- How far poor samples really lie from the train. The first GPS realism task measures it.
- What the model says about the device. Recorded samples come from phones, and nothing records the
  device's fixes. The device also reads a fix each second, against 10 s in a replay.
- What a restart does to a rider. The device crashed four times in its last 48-minute test, as
  `docs/device.md` sets out. A restart forgets every state, and can repeat a notification.
- How far train session results carry over to real trains. A train session keeps a steady speed
  along a leg and jumps at a stop, and 2.4% of steps imply over 1.5 m/s². That makes a steady-speed
  estimate look better than it is, so recorded sessions stay the check on anything about speed.
- Whether `sessionise` honours its gap. Session `8338342d` holds two samples 20 h apart under a gap
  of 600 s, and in `split` an announced start takes priority over the gap.

#### Rejected / deferred

- **A Kalman filter**, until the evaluation shows averaging limits the result. It then comes from
  a crate that builds for the device.
- **Replaying recorded sessions as the GPS model.** It keeps stretches and gaps with nothing to fit,
  but its variety stops at 32 sessions and it offers nothing to vary. If the fitted model fails its
  comparison with recorded samples, this comes back.
- **A system notification while the page is hidden.** On iOS, web notifications need a Home Screen
  web app and go through push, and an auto-lock suspends the page. A native app brings it back.
- **A site id per bridge.** If merging alerts by distance misjudges bridges, this comes back.
- **Ordering legs that share a departure by their stops.** It clears only 47 more sessions, so it
  stays in the slice "replay the trains whose legs overlap".
- **Working out each pass's instant between samples.** Moot since 2026-10-19: the nearest sample is
  at most about 5 s off, and every Motis time is a whole minute.
- **Binding `LightCone` and the state machine in `rerun-py`.** `evaluation-py` replaces it.
- **Moving `geo`, `pyo3`, and `pyo3-arrow` first.** `just outdated` lists them, and this slice
  touches them. They cannot move before SedonaDB does, so they wait for the slice "SedonaDB 0.5.0
  and the arrow generation".

### Tasks

#### Refactors / extensions

- [ ] Add a writer for gold results to `medallion`, under `run_date=<date>/run_id=<id>`, refusing a
      run that exists.
- [ ] Add `query_gold` to `crates/medallion-py`, so a notebook can read gold.
- [ ] Move `device_id` out of `matching::Session` in `session_crossings`, into the silver read that
      needs it.

#### Train sessions free of impossible steps

- [ ] Record the fastest train speed in DE and in GB, each with its source.
- [ ] Count impossible steps in `derive_train_sessions`, beside `DroppedTrips`, and log them in
      `motis_ingest`.
- [ ] Leave the time between two legs that do not join unsampled, in `NormalisedLegs::sampled`.
- [ ] Re-derive with `just silver-motis-ingest`, and compare the count with the 3,319 measured. If
      `docs/medallion.md` calls the change significant, give the datasets new versions.
- [ ] Trace the missing legs no late poll explains, and record the cause under Investigation.
- [ ] Deal with the train sessions still holding an impossible step, as the open question decides.

#### Passes for train sessions

- [ ] Add `TrainSessionCrossingRowV1` and silver `train_session_crossing_v1` to `medallion-model`.
- [ ] Derive it in `session_crossings`, with a `match_train_crossings` binary over
      `matching::passes`. Record its peak memory over 64M samples.
- [ ] Add a `silver-train-crossings` recipe, and run it from `silver-crossings`.
- [ ] Describe the dataset in `crates/session_crossings/README.md`.

#### The state machine

- [ ] Add reachability to `Prediction`, and set it in `CrowFlies`.
- [ ] Add the four states to `predictor`, reporting each state change as an event.
- [ ] Merge alerts into notifications in `predictor`.

#### Scoring recorded sessions

- [ ] Add an `evaluation` crate that replays a session's samples through a predictor and the state
      machine.
- [ ] Score each pass and each unmatched alert in `evaluation`. The outcomes are hit, late, early,
      miss, no chance, and false alarm.
- [ ] Add `EvaluationRowV1` and gold `evaluation_v1` to `medallion-model`, holding the outcome and
      the run's settings.
- [ ] Add an `evaluation-runner` crate. Its `evaluate` binary scores `session_sample_v3` against
      `session_crossing_v3`, a country at a time.
- [ ] Add a `gold-evaluate` recipe for the run.
- [ ] Add `evaluation-py`, returning one session's fixes, predictions, state changes, and alerts as
      Arrow.
- [ ] Build a notebook with the user in a live session. It shows the main measures with their
      counts, a range of N, the arrival estimate's error, and one session's replay on a map.
- [ ] Run `CrowFlies` across a range of N, as the baseline.

#### Scoring train sessions

- [ ] Read train sessions in `evaluate`, from the chosen capture, keeping those with realtime on
      every leg in `train_segment_v4`.
- [ ] Give each session's predictor only the crossings near its samples.
- [ ] Run `CrowFlies` across a range of N, and record a run's time and peak memory.
- [ ] Report train sessions apart in the notebook, by train type, each with its count.

#### Debouncing

- [ ] Count how often a crossing's state flips per pass, on recorded sessions.
- [ ] Survey how geofencing APIs debounce, and record the choice under Decisions.
- [ ] Debounce each state change, with its thresholds as parameters.
- [ ] Run the baseline again, and record what debouncing changes.

#### Speed and direction

- [ ] Add a velocity estimate to `predictor`, averaged over the last few fixes.
- [ ] Record the largest train acceleration in DE and in GB, each with its source.
- [ ] Add `LightCone` to `predictor`, with the cone's width as a parameter.
- [ ] Count the passes `LightCone` rules out within N, on recorded and train sessions.
- [ ] Score it across a range of N and of cone widths, against the baseline.

#### GPS realism

- [ ] Measure how far poor samples lie from where the train was, in a notebook with the user. Use
      the rail, and the good samples either side.
- [ ] Fit the model to recorded samples, in `evaluation`.
- [ ] Add GPS error to a train session's samples at replay, repeatable for a given run and session.
- [ ] Add a flag to `evaluate` choosing the model, with flags overriding its fitted values.
- [ ] Plot generated samples beside recorded ones in the notebook: accuracy, stretch lengths, gaps,
      and speed between samples. If they disagree, revise the model or record the difference.
- [ ] Compare `CrowFlies` on noisy train sessions with `CrowFlies` on recorded sessions. If the
      noise does not account for the gap the preview shows, the model is missing something.
- [ ] Run `CrowFlies` and `LightCone` across a range of N with the model on.
- [ ] Choose the velocity estimate's accuracy limit from those runs.
- [ ] Research how to tell a real second pass from GPS jitter. Compare how closely a real train can
      come back with how uncertain a session's samples leave the train. Look for prior art, such as
      scale space in images. Test against noisy train sessions. Record the findings under Decisions.
- [ ] Choose the settings for the shells from those runs, and record the evidence.

#### Notifications

- [ ] Run `LightCone` and the state machine in `crates/platform-core/src/app.rs`, with the chosen
      settings.
- [ ] Time one GPS update on a release device build, against the second between fixes. The user
      flashes it.
- [ ] Add a notification effect to `Effect`, raised once per bridge.
- [ ] Show it as a banner in `lookout-predictor.js` and on the M5 panel.
- [ ] Hold a screen wake lock on `/live`, as `/record` does.
- [ ] Draw each crossing's state in the web picture and on the M5 panel.
- [ ] Describe the notification and the states in `docs/web.md` and `docs/device.md`.
- [ ] Build both shells, and flash the device. The user rides with it.

#### Wrap-up

- [ ] Add the evaluation and `train_session_crossing_v1` to the derivation graph in
      `docs/architecture.md`.
- [ ] Describe what a late poll loses in `docs/motis.md`.
- [ ] Fold what holds of `docs/2026-08-01-evaluation.md` into a durable doc, and delete it. Its
      "operating point" becomes the chosen settings.
- [ ] Run `just test-no-docker` before every commit. The user runs `just test`.

### Investigation

Measured on 2026-11-03, with throwaway scripts over the store as it stood.

#### Impossible steps in train sessions

An impossible step is two samples 10 s apart that imply over 320 km/h. In the whole capture, 3,319
of 163,402 train sessions hold one, or 2.0%. Among sessions with realtime on every leg, 1,098 of
33,830 do, or 3.2%, all of them regional. Such a step gives the predictor an absurd speed, and skips
the passes it jumps over.

Three causes account for all but one of those sessions. A session can have more than one.

| Cause | Sessions |
| --- | --- |
| A leg is missing, so the next leg starts somewhere else | 2,911 |
| A leg over 890 m takes no time, and 97% of such legs are realtime | 412 |
| A leg's own times imply over 320 km/h | 98 |

##### Why legs go missing

Polls run every 10 minutes, and each asks for legs running within 5 minutes either side of it. No
window overlaps the next, so a late poll leaves a hole. Ten polls ran late, the worst by 150
minutes. Of the 3,290 jumps over 890 m, 65% meet such a hole, rising to 96% on 21 October and 92% on
22 October.

The rest are not traced. On 18 October, almost no jump meets a hole, which points to a delay moving
a leg between two windows. Some jumps of about 15 km recur daily, which points to the rule that
keeps only legs wholly within the country.

##### What a fix buys

These are estimates from the legs, not from a re-derivation.

| Change | Sessions left with an impossible step |
| --- | --- |
| None | 3,319 |
| Leave the time between two legs that do not join unsampled | 628 |
| Also give legs that take no time some time | 262 |

#### What realtime changes

Of the legs polled on 19 October, realtime covers 96% of regional, 16% of high-speed, and 7% of
long-distance legs. 74% of realtime legs keep their scheduled duration, and 96% stay within a minute
of it. Regional departures run late by a median of 1 minute, and 8 minutes at the 90th percentile.
Realtime therefore moves a train in time far more than it changes its speed.

On the days with realtime, ingest also drops about 15% of trips as `overlapping`, most of them where
a later capture moved a leg's times. Realtime-only scoring therefore leaves out many delayed trains.

#### How recorded samples vary

These figures cover the 32 recorded sessions that moved. They hold 6,269 samples over 23.1 h. A good
sample reports accuracy up to 100 m, and a poor sample reports more. A stretch is an unbroken series
of good or of poor samples.

| | Good | Poor |
| --- | --- | --- |
| Share of samples | 80% | 20% |
| Reporting a speed | 95.7% | 5.5% |
| Speed between samples, median | 27 m/s | 16 m/s |
| Speed between samples, 90th percentile | 44 m/s | 139 m/s |
| Speed between samples, 99th percentile | 66 m/s | 736 m/s |
| Time since the last sample over 20 s | 2% | 32% |
| Time since the last sample over 60 s | 0.6% | 5.5% |
| Length of a stretch, median | 9.5 samples | 2 samples |
| Length of a stretch, mean | 40 samples | 11 samples |
| Length of a stretch, longest | 365 samples | 250 samples |

Only 2.7% of samples report between 100 m and 500 m, which gives the model its two states. Among
poor samples, the commonest values are exactly 2,000 m and 1,414 m. 7 of the 32 sessions are
mostly poor, and they hold 82% of all poor samples.

Reported accuracy holds for good samples and overstates poor ones. Of samples reporting up to 10 m,
67% lie within that distance of the rail. Samples reporting 1 km to 5 km lie a median of 176 m to
286 m from the rail. That measure misses error along the track. On the 134 poor samples with good
samples either side, the median error is 110 m for those reporting under 500 m. For those reporting
more, it is about 500 m to 800 m, on 24 samples.

### Observations

#### A rough preview

On 2026-11-03, a throwaway script replayed `CrowFlies` and some direction rules at N of 60 s, with
no debouncing. It left out train sessions with an impossible step. It shows what to expect, and it
is not the result.

A useful alert comes 30 s to 2 min before the pass. A false notification is a group of alerts at one
moment with no pass within 5 minutes. "Ahead" means within 90° of the direction of travel. A lane
keeps crossings ahead and within that distance to the side.

| Sessions | Rule | Passes with a useful alert | False notifications an hour |
| --- | --- | --- | --- |
| Recorded, every fix | Crow flies | 74% | 19.7 |
| | Ahead | 74% | 7.8 |
| | 45° cone | 72% | 6.0 |
| Recorded, ignoring fixes over 100 m | Crow flies | 80% | 9.8 |
| | Ahead | 76% | 3.5 |
| | 45° cone | 72% | 2.9 |
| Realtime regional, 19 October | Crow flies | 91% | 11.0 |
| | Ahead | 90% | 2.8 |
| | 45° cone | 89% | 2.1 |
| | 20° cone | 77% | 1.1 |
| | 500 m lane | 88% | 1.8 |
| Timetable regional, 22 October | Crow flies | 92% | 11.1 |
| | Ahead | 92% | 2.7 |
| | 45° cone | 92% | 2.0 |
| High-speed, 22 October | Crow flies | 92% | 19.3 |
| | Ahead | 91% | 8.2 |
| | 45° cone | 90% | 6.0 |
| Long-distance, 22 October | Crow flies | 94% | 13.7 |
| | Ahead | 93% | 4.6 |
| | 45° cone | 93% | 3.2 |

What it suggests:

- Knowing what lies ahead does most of the work. It cuts false notifications by roughly 60% to 75%,
  and loses almost no passes. A cone narrower than about 45° loses passes quickly.
- Ignoring poor fixes halves false notifications on recorded sessions.
- Realtime and timetable trains score alike.
- Recorded sessions score well below train sessions. GPS realism has to reproduce that gap.
