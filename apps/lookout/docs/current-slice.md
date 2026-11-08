# Current Slice: Evaluation framework with associated Crow Flies predictor improvement

### Target

I have some larger ideas for how to come up with a good predictor (see [notes](2026-10-03-observational-predictor.md)), but for now I want to:
* improve the predictor in a minimal way
* measure that improvement robustly

I'd like to go from a very simple Crow Flies predictor I have now, which effectively just shows distance to nearby crossings, to one that takes into account current speed and average direction, and predicts when a crossing will be passed in next N minutes. Effectively a "Light Cone" approach.

The evaluation method should be focussed on being well-understood and robust, even if it does not simulate all aspects of reality. More complete extensions can be reserved for later slices.

### Straw Man

#### What is a Prediction?

We have to become a lot crisper than previously on what a prediction means. Right now the display just shows a radar style view which isn't really making a prediction and is instead just a map of nearby crossings. 

My suggestion is that, for each crossing, we need to have a state-machine, something like:

States:
* `OutOfReach`: this is maybe an implicit state that any crossing sits in, if it is just in the pointset data, and isn't reachable at all
* `CandidatePass`: this is when it becomes nearby enough to be considered as a possible crossing
* `PredictedPass`: this is when we expect we will actually pass by this crossing in the next N minutes
* `ActualPass`: we just actually passed the crossing

Transitions:
* `OutOfReach` <-> `CandidatePass`: a crossing can oscillate in and out of being a candidate
* `OutOfReach` -> `PredictedPass` or `CandidatePass` -> `PredictedPass`: this is us making a prediction, and this transition should lead to a notification of some kind to the user
* `PredictedPass` -> `CandidatePass` or `PredictedPass` -> `OutOfReach`: this counts as a failed prediction
* `PredictedPass` -> `ActualPass`: a successful prediction.
  * A crossing stays in the `ActualPass` state until we've gotten some distance away from it to avoid re-notifying repeatedly.
  * In general, the state transitions should probably have some sort of debouncing or hysteresis to avoid frequent flips in and out of states.
* `ActualPass` -> `PredictedPass`: it's always possible we can pass somewhere multiple times, so this counts as a new prediction
* `ActualPass` -> `CandidatePass`: still within reach but not currently predicted
* `ActualPass` -> `OutOfReach`: it's now some distance from us and we don't even think it is a candidate

In this framework, depending on how clever the predictor is, we may be able to display a crossing but still label it as OutOfReach. This could be, for example, if we decide it is technically close by but we are not going to reach it because it's not on our current track. However, any predictor that can do that is likely out of scope of this slice.

#### Moving from a CrowFlies to a LightCone Predictor

The cleverness of the predictor we do in this slice should be limited to using a current-speed/direction + Light-cone approach i.e. predict what we will reach based on where we will pass by based on current velocity vector.

This can start with a low-level of cleverness about current speed/direction (i.e. just literal current speed) and then start building up an overall speed/direction based on an average over last few fixes and/or on speed reported by the GPS device.

I suspect on Web we could benefit from the cleverness of the GPS baked into the phone, as it's probably doing something clever, but I'd prefer not to use that and instead do it ourselves, as then we have consistent performance across platforms. I am thinking here of some sort of Kalman-filter or similar; however that's possibly over-kill for this slice.

#### Evaluation method and modelling approach

The method should be informed by the [research](2026-08-01-evaluation.md) done previously.

We should drive a predictor based on fake train data as if it was receiving it live, and then assess it based on ground truth of what was actually passed.

This should use the motis data we have recorded i.e. we should replay `train_session_sample_v2`, with some caveats. The intent of the previous slice was to record a full weeks worth of train data for Germany, but unfortunately part-way through we lost `realtime` feeds [without realising it](completed-slices.md#caveat-realtime-stops-partway-through-the-capture). 

Some [analysis](../notebooks/evaluation/explore1.py) shows which `trip_id`s on which `polled_date`s have > 90% coverage of realtime datapoints. It's about 40,000 trips clustered around 2026-10-18 and 2026-10-19. We should implement this 90% coverage filter by re-doing the derivation of `train_session_sample_v2` so that it contains a `realtime` column and extend `train_session_v2` to have a `realtime_pct` which allows us to apply the filter i.e. we'd only include sessions whose coverage was >= 90%.

The underlying data may require some further cleaning to remove any other anomalies that exist. As long as this doesn't massively affect coverage then we can adopt a bias to discard invalid data instead of trying to fix it. This may have to happen at the session level i.e. we may have to discard whole sessions if some percentage of it (e.g. 10%) is invalid.

We will also want to do a version of `session_crossing_v3` but for train sessions, `train_session_crossing_v1`. We'll need this to assess Precision i.e. what the predictor said it would reach soon, vs what it did.

To measure recall, one approach we could take is to have an "oracle" predictor to compare against: we could take the actual crossings the train passed and works backwards to when a perfect predictor would have said it was coming. This allows us to assemble a stream of crossing state updates that would be the best score. This may also flush out if there are some constraints or ambiguities in our system which means even an oracle with perfect foreknowledge can't get a perfect score on precision and recall.

Hopefully these `realtime` samples will be enough samples to give a good statistical evaluation, but if not, we can relax our `realtime` constraint and instead use all of the sessions we have.

We don't need to model gps accuracy in this slice, but it can be an extension we attempt if things go well. The sorts of thing we could do here would be to use the real recorded sessions from being on a train to build a distribution of gps accuracy and sample from that model. This could then be layered on top of the train position as an error signal to perturb it.

#### Visualising in Crux App on Web and M5

The CrowFlies and LightCone Predictors will need to make explicit the states of each crossing as they see them.

These only have to be minimally supported in the Crux Apps e.g. showing a different shape for each crossing either in the Canvas view, or as some ASCII-art on M5. They don't have to be updated to manifest a notification from the internal predictor as a user-visible notification.

#### Style of development and how to record evaluations

I generally prefer a Walking Skeleton approach and I'd like to apply that here, alongside some of the themes of explainability from [recent investigations](2026-10-18-demonstrable-behaviour.md). This means that I'd like to have a very basic evaluation in place, starting with a small amount of samples, say `1000`. This allows us to go through an inspectable series of steps that show if the method is working before extending to longer runs.

This also means we don't have to implement the LightCone predictor at start. That can come after we have a proper evaluation of CrowFlies in place.

The output of evaluation should live in `gold` as tables recording things like predictor name + parameters, evaluation method, sample sizes, and random seeds, as well as output. So, we probably want to have a mix of tables describing the evaluation run as well actual results. These can be summary distributions.

However, I probably also want the data in detail about what the predictor predicted and when, perhaps recorded as an `evaluation_session`, also in `gold`. This can perhaps record the state transitions over time. This can be inspected for debugging to check everything is working, and also to replay edge-cases or to visualise where there are hotspots of prediction failures. If we are wanting to replay over time, rerun may be a good choice, but a notebook is also fine.

Overall this means that Tasks should be broken down into major stages with each stage delivering some aspect of
* derivation of data for sessions
* driving predictor
* evaluating it
* visualising it

### The measure

A **notification** is a crossing's entry into `AlertedPass`. It claims that the train passes the
crossing within the horizon N.

A notification's **lead time** runs from it to the next pass of its crossing. Each pass credits the
notification that scores worst for it, and that notification's lead time sets its class:

| Class | Lead time | Score |
| --- | --- | --- |
| Hit | from 30 s to N | 1 |
| Last moment | under 30 s | 0.25 |
| Too early | over N | falls from 1 to 0.05 across the ramp, and stays 0.05 past it |

The **ramp** starts at N, and its width is a setting each run records, as are the scores. The
first runs use a ramp one minute wide, so a notification 5:30 ahead of its pass scores about 0.53.

Every other notification scores 0. A **repeat** is one whose pass credits another notification. A
**false alarm** is one with no pass of its crossing still to come. A **miss** is a pass that credits
no notification.

- **Precision** is the total score over notifications, repeats included.
- **Recall** is the total score over the scored passes.
- **False alarms per hour** is false alarms over the hours replayed.
- **Lead time** is reported as a distribution, over the notifications the passes credit.

[The research](2026-08-01-evaluation.md#outcome-classification) borrows its early and late classes
from punctuality, where a late train still carries its rider. These classes depart from it, since a
crossing passed is gone: nothing after the pass earns credit. Credit is also graded rather than
counted. Transitions into the other states are recorded and not scored. Entries into `ActualPass`
are among them, since the ground truth decides whether a pass happened.

### The datasets

The slice adds three silver datasets and five gold ones. Gold partitions by `run_date` and then
`run_id`, as [medallion.md](medallion.md#partitioning) lays out evaluation results.

```mermaid
erDiagram
    train_session_v3 ||--|{ train_session_sample_v3 : "sampled as"
    train_session_v3 ||--o{ train_session_crossing_v1 : "passes"
    water_crossing_v2 ||--o{ train_session_crossing_v1 : "passed in"
    evaluation_run_v2 ||--|{ evaluation_session_v1 : "replays"
    train_session_v3 ||--o{ evaluation_session_v1 : "picked for"
    evaluation_session_v1 ||--o{ evaluation_transition_v1 : "records"
    evaluation_transition_v1 ||--o| evaluation_outcome_v1 : "notification scored as"
    train_session_crossing_v1 ||--o{ evaluation_outcome_v1 : "scored in"
    evaluation_run_v2 ||--|| evaluation_summary_v1 : "sums up"
```

- `train_session_v3` and `train_session_sample_v3` are the v2 datasets with
  `realtime_percentage` and `realtime` added.
- `train_session_crossing_v1` holds each pass of a train session, one per visit within the match
  radius.
- `evaluation_run_v2` holds a run's predictor and settings, pass detector, float type, scores and
  ramp, evaluation version, country, filter, sample size, seed, and bootstrap resamples. It also
  holds the run's code version, and a map from each silver dataset the run reads to its
  fingerprint. A dataset added to or removed from the run's inputs needs no new column.
- `evaluation_session_v1` holds each session a run replays, with its hours, so a session with no
  transitions still counts.
- `evaluation_transition_v1` holds each transition. A crossing whose state stays the same between
  two samples writes nothing.
- `evaluation_outcome_v1` holds each notification with its class and score, and each miss.
- `evaluation_summary_v1` holds a run's measures and their intervals, with its counts of sessions,
  passes, and notifications.

### Decisions

Decisions taken before starting, as each changes what gets built, confirmed on 2026-11-04.

- **The evaluation is a chain of named steps in Rust, which Python runs and one CLI composes.** Each
  step takes and returns tables that map to Arrow, with geometry as GeoArrow. A notebook runs any
  step through a Python extension, for quick iteration and inspection. The CLI runs every step in
  order, or any one alone, against silver and the gold earlier steps wrote. Its full run is itself a
  step the extension exposes, so a notebook runs what the CLI runs. This applies the named steps of
  [the demonstrable-behaviour assessment](2026-10-18-demonstrable-behaviour.md#a-derivation-reports-its-steps).
  It beats a binary alone, whose steps a notebook cannot inspect, and a notebook alone, which puts a
  derivation in Python.
- **A step reads silver through `Read<T>`, a typed read whose operators the engine runs.**
  `Query::read_of_country` returns a `Read<T>` over one country's partition. A step states its
  filter, sort, and limit as the engine's own expressions, and gets back rows of `T`, read from
  `T`'s columns alone. The engine skips each file whose statistics rule the filter out. It drops
  rejected rows as they stream, so a step holds only the rows it keeps. This beats fetching every
  row to filter in Rust, which holds the whole dataset at once. It also beats SQL text, which a step
  assembles by formatting strings. Decided on 2026-11-06, when `pick` was built.
- **Each phase explores in a notebook of its own.** A phase that explores starts the next
  `notebooks/evaluation/exploreN.py`, rather than adding to an earlier phase's. A notebook may
  duplicate another's cells. The wrap-up folds them into one or more notebooks that demonstrate the
  evaluation's behaviours or steps, as
  [the demonstrable-behaviour assessment](2026-10-18-demonstrable-behaviour.md) sets out. This beats
  one notebook grown across phases, where a question already answered sits among the next phase's
  work.
- **Precision counts each notification.** Every entry into `AlertedPass` is one notification,
  scored as [The measure](#the-measure) sets out. A predictor that flickers pays for each flicker,
  as a rider would. Recall counts each ground-truth pass once. This beats one outcome per (session,
  crossing) pair, taken from its first notification, as
  [the research](2026-08-01-evaluation.md#unit-of-counting) proposes. That leaves every repeat
  unscored.
- **A notification earns credit by its lead time.** It earns full credit from 30 s to N before its
  pass, a heavy penalty under 30 s, and almost nothing past the ramp beyond N. A notification with
  no pass of its crossing still to come earns nothing. A rider needs warning in time to act, and an
  early warning goes stale. A crossing passed is gone, unlike a late train, which a rider can still
  board. This beats a hit anywhere within N, which credits a notification seconds before its pass,
  and nothing a moment past N.
- **Past N, the score ramps down rather than steps.** CrowFlies and LightCone both aim at N, so
  their notifications bunch at that edge. Say CrowFlies estimates a pass 4:58 away, and notifies. On
  track 2% longer than the straight line, the pass comes 5:04 later. A step at N would cut that
  notification's score from 1 to 0.05, though a rider can barely tell 4:58 from 5:04.
- **One state machine, the tracker, lives in `predictor`.** The tracker turns predictions and
  passes into transitions, applying the legal transitions. The oracle, CrowFlies, and LightCone
  therefore share one set of transition rules. This beats a state machine per predictor, whose
  rules can drift apart.
- **A prediction carries a confidence, low or high.** The tracker maps low to `CandidatePass` and
  high to `AlertedPass`. `AlertedPass` replaces `PredictedPass`, so "predict" names only what a
  predictor does. A predictor then forecasts and never chooses a state. This beats a predictor
  choosing each crossing's state, which leaves it to declare a pass it can only forecast. Changed
  on 2026-11-05, when the tracker was built.
- **A pass detector, not a predictor, reports each pass.** One trait has two implementations. The
  apps run the online one, which reports a crossing within a pass radius of the latest sample. A
  replay runs the offline one, which reports the passes in `train_session_crossing_v1`. An
  evaluation of its own compares the two. A crossing stays in `ActualPass` while the detector
  reports it, and leaves on the next prediction after that. A pass in ground truth lasts one
  sample, so `ActualPass` lasts one round. This beats a predictor reporting the pass, which mixes
  forecasting a pass with observing one. Changed on 2026-11-05, when the tracker was built.
- **A predictor debounces its own predictions.** The predictor knows how it predicts, so it knows
  which debouncing suits it, and a predictor can need none. The tracker applies each prediction as
  it arrives. This beats debouncing in the tracker, which imposes one rule on every predictor.
  Debouncing waits until after LightCone, as an optional experiment. Changed on 2026-11-05, when
  the tracker was built: the tracker held the debouncing until then.
- **Each state is a type, so an illegal transition does not compile.** `CandidatePass`,
  `AlertedPass`, and `ActualPass` are types, and `OutOfReach` is a crossing the tracker holds
  nothing for. A transition is a method that consumes one state and returns the next. `ActualPass`
  has no constructor outside its module, and only `AlertedPass` has a method returning one. The
  tracker keeps each crossing's state in an enum over the types, as
  [Hoverbear's enum wrapper](https://hoverbear.org/blog/rust-state-machine-pattern/) does. Its
  `match` can call only the transitions a state has. This follows
  [the typestate pattern](https://cliffle.com/blog/rust-typestate/). It beats a check at run time,
  which a test can only sample.
- **The first run replays 1000 sessions.** They are picked at random, under a recorded seed, from
  the sessions with `realtime_percentage` ≥ 90. A session is the unit a predictor replays. It beats
  1000 sample rows, which hold a handful of sessions, and 1000 passes, which fix recall's
  denominator rather than the sessions.
- **`pick` picks in the engine, by a hash of each session's id under the seed.** The engine keeps
  the sessions the filter passes, orders them by the SHA-256 of the seed and the session's id, and
  keeps the first N. It returns those N in order of id. Ordering by a salted hash is a standard way
  to pick without replacement in SQL. The sort and the limit run together, so the engine never
  holds every eligible session. What a seed picks depends on SHA-256 alone, so no library upgrade
  changes it. This beats picking in Rust with a seeded generator, which holds every eligible
  session, and whose picks change with the generator's version. Decided on 2026-11-06.
- **A run replays one country's sessions.** The store lays train sessions out by country, each
  country in its own zone, and a read names the country it reads. The crossings a replay matches
  against are laid out the same way. `evaluation_run_v1` records the country. Decided on
  2026-11-06.
- **A run's id is a hash of its settings and inputs.** The id is a v5 UUID over everything
  `evaluation_run_v1` records of the run, its input versions included. The same settings over the
  same inputs give the same id, and gold refuses a run it already holds, so each run is written
  once. A code change or a re-derived dataset changes an input, so a rerun after either gets a
  new id. This
  beats the instant the run started, which writes a copy of each rerun, and a random UUID, which
  does the same and says nothing of the run. Decided on 2026-11-06.
- **A run's inputs are its code version and a fingerprint of each silver dataset it reads.** A
  fingerprint hashes the footer of each of the dataset's files, which medallion reads through its
  object store. A re-derive that writes the same rows keeps the fingerprint, and so the run's id.
  This beats the code alone, which refuses a run against re-derived data. It also beats the
  fingerprints alone, which refuse a rerun after a code fix. Decided on 2026-11-06.
- **A run names its code by git's hashes of the code, not by a commit.** The code version joins the
  hashes of `crates/`, `Cargo.toml`, and `Cargo.lock` at HEAD. A commit of runs, data, or docs
  leaves it unchanged, so a run committed to gold does not change the next run's id. This beats
  the commit, which every such commit changes. Changed on 2026-11-07, when a notebook could not
  match the CLI's run across a commit of that run.
- **A run is written only from committed code.** `evaluate` refuses to begin a run from a build
  with uncommitted changes to its code, so every run in gold names code a checkout holds. A step
  run alone also refuses once the code version or a silver dataset's fingerprint differs from its
  run's. This beats hashing the uncommitted diff into the version, which keeps no record of the code
  a run used. Decided on 2026-11-06.
- **A run records the version of the evaluation's rules.** `evaluation_version` is a name such as
  `replay-v1`, changed by hand when a rule of the evaluation changes, such as how a repeat scores.
  It takes the place of the method the datasets once listed. Decided on 2026-11-06.
- **The steps take and build `medallion-model`'s rows.** `pick` already brings medallion,
  DataFusion, and SedonaDB into `evaluation`. With them linked, a change to a step rebuilds
  `evaluation-py` in 20 s. Each gold table then has its columns defined in one crate, with no test
  holding two definitions equal. Decided on 2026-11-06.
- **A run started from Python writes gold, as a run of the CLI does.** A notebook writes through
  `medallion-py`, and the CLI through `medallion`'s writers, so both use one implementation. A run
  iterated on in a notebook then leaves the same record as any other. This beats a notebook that
  only inspects, whose runs a person repeats through the CLI to keep.
- **N is 5 minutes.** A rider needs a few minutes' warning of a pass, to get a camera out or tell a
  friend. Five minutes ahead leaves that much, even for a notification a minute or two short of it.
  It beats the minute that [target.md](target.md) gives, which leaves too little time to act. The
  rider's need fixes N, so no sweep varies it.
- **A crossing reaches `ActualPass` only from `AlertedPass`.** `ActualPass` marks an alert that
  came true, so a pass with no alert leaves its crossing in `CandidatePass` or `OutOfReach`. An
  alert that starts in the round of its pass comes true at once. The tracker records both
  transitions at that round, and the notification is last moment, with a lead time of 0. A pass in a
  session's first minute leaves too little time to notify it 30 s ahead. A predictor first needs
  samples to measure a speed. Scoring therefore truncates that start. Such a pass goes unscored, as
  does a notification it credits. The predictor still replays those samples, so it has a speed once
  scoring starts. This beats marking a pass with no alert as `ActualPass`, which records a success
  nobody earned. Changed on 2026-11-06: the oracle's check found an alert that started at a pass
  left open until the crossing's next pass, which then counted as a miss.
- **A replay runs in `f32` by default, and its run records the float type.** The apps run the
  predictor in `f32`, so a replay in `f32` measures the program they run. In Germany, rounding to
  `f32` moves a point by up to 0.21 m, so predictions differ only where a distance sits at an edge. A
  run in `f64` shows how much that changes. This beats `f64` alone, the store's float type, which
  measures a program no app runs.
- **A dwelling sample takes the arriving leg's `realtime`.** A train dwells at a stop between one
  leg's arrival and the next leg's departure. Only a wrong arrival time misplaces a dwelling sample,
  since a train that leaves late is still at the platform. A late departure falls on the departing
  leg's samples, which carry that leg's flag. `position_at` already places a dwelling sample by the
  arriving leg. The derivation fixes the choice, so no run varies it. This beats the departing leg,
  whose missing `realtime` would mark a correct dwell unreliable.
- **Precision, recall, and false alarms per hour each carry an interval, from a bootstrap over
  sessions.** `summarise` resamples as many sessions as the run replayed, with replacement, and
  works out each measure over the resample. Over many resamples, the middle 95% of each measure's
  values is its interval. The run records the number of resamples, and its seed fixes them. A
  session's notifications rise and fall together, so resampling notifications would give intervals
  too narrow.
  Sessions on one line repeat each other in the same way, so these intervals still come out
  narrower than the truth. This beats a textbook formula, which assumes independent yes-or-no
  outcomes rather than graded scores.
- **`summarise` keeps its own bootstrap, and a test holds it to SciPy's.** No Rust library
  resamples whole sessions for a statistic of several columns, with a seed that holds across
  versions. `linfa` and `ndarray-rand` only draw rows, on older versions of `rand`, and
  `anofox-statistics` resamples one number per unit. `scipy.stats.bootstrap` does all of it, so
  `evaluation-py`'s tests require each end of each of `summarise`'s intervals to lie within 5% of
  the interval's width of SciPy's. This beats computing the intervals in Python, which leaves the
  CLI without them or embeds an interpreter in it. Decided on 2026-11-07.
- **Two predictors differ only where their intervals do not overlap.** Overlapping intervals mean
  the run cannot tell the two apart, not that they are equal. The rule is cautious: it rarely calls
  a false gain real, and it can miss a small real one. More sessions narrow every interval. The run
  over every session the filter keeps, about 40,000, gives intervals about six times narrower than
  1000 sessions do. This beats a paired bootstrap of the difference, which needs a step and a
  dataset of its own.
- **A person chooses the apps' predictor and its settings from a report a skill writes.** The
  skill, `choose-predictor`, reports the runs in gold as a dated doc in `docs/`, so the review sits
  in a diff. The discussion it opens ends in a predictor and settings, which a code change puts in
  `platform-core`. The choice weighs what the numbers alone leave open, such as recall against false
  alarms per hour. The report shows every run, so the choice stands beside the runs it passed over.
  A later predictor goes through the same review. This beats a rule fixed before the results, which
  cannot weigh a trade-off nobody foresaw.
- **The cleaning rule discards a whole session that fails either of two checks.** A session fails
  if any sample lies outside its country, or if any sample implies a speed over 100 m/s.
  `motis::train_sessions` applies both in Rust, as drop reasons beside its existing four, and counts
  each. A country's land and maritime areas, the ones that already place a point, decide what lies
  inside it. The speed check catches a jump, a single 10 s step of tens of kilometres onto a stop.
  [Investigation](#investigation) gives the evidence for both. Discarding fits the Straw Man's bias
  towards discarding invalid data, and the checks cost 2% of sessions. This beats marking rows for
  `pick` to filter, which adds a v3 column that every reader has to know to apply.

#### Consequences elsewhere

- **`train_session_v2` and `train_session_sample_v2` give way to v3.** Any change to a declaration
  makes a new version, an added column included. `motis_ingest` writes v3 alone, and the v2 row
  types go.
- **The matching's session loses its device.** `session_crossings::matching::Session` holds a
  `device_id`, which a train session lacks. The device moves to the rows that `silver.rs` builds.
- **Gold holds datasets of rows for the first time.** [medallion.md](medallion.md#partitioning)
  partitions evaluation results by run, and nothing writes them yet. `medallion`'s writers and its
  `Query` gain gold, and `medallion-py` gains `query_gold` and `write_gold`.
- **A trip leaving its country is no longer written.** `motis::train_sessions` now writes such a
  trip under the country it starts in, as
  `a_trip_leaving_every_known_country_is_written_under_the_one_it_starts_in` tests. Under the
  cleaning rule, it drops the trip instead.
- **CrowFlies' radius has to reach N ahead.** Its 5 km covers 5 minutes only at 60 km/h or slower,
  so the sweep varies the radius.

#### Rejected / deferred

- **A Kalman filter on the velocity.** Deferred, since the Straw Man judges it more than this slice
  needs. A train sample carries no noise to filter, so it waits on a model of GPS error.
- **A model of GPS error.** Deferred: the Straw Man's extension for a slice that goes well. It
  perturbs each train position with accuracy sampled from the recorded sessions.
- **The phone's own speed and heading.** Rejected, so that every platform runs one velocity
  estimate.
- **Interpolating each pass's instant along the path**, as
  [the research](2026-08-01-evaluation.md#ground-truth) proposes. Deferred:
  `train_session_crossing_v1` takes the nearest sample, as `session_crossing_v3` does. Samples lie
  more than twice the match radius apart in 0.18% of steps, which loses about 0.1% of passes, an
  acceptable share. It comes back once a run's scores trace to a pass lost that way.
- **Rerun as the viewer of a run.** Deferred, since the notebook draws a session's transitions over
  time. It comes back once watching one replay at speed is wanted.
- **A state machine crate.** Rejected. `statig` and `rust-fsm` check each transition as it runs,
  which the types replace. `statum`, `sm`, and `typestate` generate the types from a macro, and
  four states read more plainly written out.
- **A test that an illegal transition fails to compile.** Rejected: four states have few enough
  transitions to inspect by eye. Such a test needs [`trybuild`](https://docs.rs/trybuild), since a
  `compile_fail` doctest passes on any compile error, a typo included. It comes back once the
  transitions outgrow an inspection.
- **A probabilistic model of how trains turn.** Deferred: ballpark odds of the extremes bound the
  cone's angle. It comes back once LightCone's misses trace to turns those odds call rare.
- **A ramp at the 30 s edge.** Deferred: no predictor aims at 30 s, so last-moment notifications
  spread out rather than bunch at it. It comes back once a run shows them bunching there.
- **A notebook as `choose-predictor`'s report.** Rejected: a dated doc keeps the review in a diff.
  It comes back once each review needs the maps that the evaluation's notebooks draw.
- **Resampling whole lines rather than sessions.** Deferred: resampling lines needs each session to carry
  its line, and no dataset holds one yet. A line with a train every 20 minutes gives dozens of
  near-identical sessions a day, so intervals over sessions come out narrower than the truth. Each
  leg in `motis_segment_v3` carries a `route_name`, not yet checked as the name of a line. It comes
  back once two predictors' intervals clear each other only narrowly.
- **A straightness check on each session.** Deferred: the two checks of the cleaning rule leave
  some sessions that look bogus on a map, such as one running straight between stops. Finding them
  needs a measure of how far a path strays from track, such as the `off_track_share` that
  [the demonstrable-behaviour assessment](2026-10-18-demonstrable-behaviour.md#investigation)
  adopted. It comes back once a predictor's misses or false alarms trace to such a session.

### Investigation

What `notebooks/evaluation/explore2.py` established about `train_session_sample_v2`, on 2026-11-04.
The high tail is the 1% of sessions with the most samples, about 1,700 sessions.

- **Samples fall 10 s apart.** Every step between consecutive samples in the high tail takes 10 s.
  The median step covers 317 m.
- **A bogus session jumps once.** The user picked 24 bogus-looking sessions from a map of the high
  tail. In 23 of them, a single step covers 38–236 km and lands on a stop. In the other sessions
  of the high tail, the median longest step is 496 m.
- **A speed limit separates the jumps.** In the high tail, no session without a jump exceeds
  71.5 m/s. Every session with one exceeds 145 m/s. A limit of 100 m/s sits in that gap.
- **The one pick without a jump leaves Germany.** It runs straight between two stops, and comes
  from the older `delfi-2026-10-10.route-shapes` source. 620 of its samples lie outside Germany's
  land and maritime areas. No other session in the high tail has a sample outside them.
- **The two checks flag every pick, and some sessions that look valid.** In the high tail, they
  flag all 24 picks and 13 more. Some of those 13 look valid on the map.
- **Across every session, the checks discard 2%.** Of 165,606 sessions, 3,342 fail the speed check
  and 8 lie partly outside Germany. None fails both. The 3,350 hold 1,796,894 of the 64,232,436
  samples. Some sessions that look bogus on the map pass both checks.
- **Two stops alone marks no fault.** A check on sessions with exactly two stops flagged sessions
  that look valid on the map, and was dropped.

What measuring the matching of train sessions to crossings established, on 2026-11-05, over the
162,256 sessions, 62,435,542 samples, and 5,749 crossings in Germany.

- **Gathering each session's samples into one array exhausts memory.** `match_crossings` gathers
  them with `array_agg`, grouped by session, and reached 52 GB before it was stopped. Plain
  aggregates over the same samples take about a second and 0.4 GB.
- **Nearly every distance measured is unneeded.** The matching keeps each crossing within the radius
  of the box bounding a session's samples. That keeps 11.5 million pairs of a session and a
  crossing, and measures 16.8 billion distances from a sample to a crossing. About 7.9 million
  pairs of a sample and a crossing lie within 250 m, estimated from 2,000 random sessions.
- **A box filters a long trip poorly.** The median session keeps 14 crossings and the largest keeps
  3,839, since a trip across the country spans much of it.
- **SedonaDB's indexed spatial join is out of reach.** Its `spatial-join` feature depends on GEOS
  in 0.4.1, as [the crossings port](2026-09-27-crossings-in-rust.md) found in 0.4.0. Without it, a
  join on `ST_DWithin` compares every pair, 3.6 × 10¹¹ distances.
- **Ranking the samples by time costs 6.3 GB.** `ROW_NUMBER() OVER (PARTITION BY session_id ORDER
  BY t)` over every train sample takes 5.6 s, and its sort holds every row at once.

What `notebooks/evaluation/explore2_spacing.py` established about the spacing of samples, on
2026-11-05, over the 34,521 sessions with `realtime_percentage` ≥ 90 and their 11,069,925 samples.

- **Each step spans 10 s and a median of 188 m.** The 99th percentile is 408 m, and the longest step
  is 998 m, just under the cleaning rule's 100 m/s.
- **0.18% of steps exceed twice the radius.** They fall in 362 sessions, on fast lines. 0.055% of
  the distance travelled lies more than the radius from every sample.
- **The paths come within the radius of a crossing 439,004 times, and 2,001 record no pass.** A path
  here is the line through a session's samples in order.
- **489 misses lie on the train's own track, and the spacing causes each.** Each lies within 50 m
  of its path, beside a step longer than twice the radius. They are about 0.1% of the 437,003
  pairs of a session and a crossing passed.
- **1,334 misses lie beside the track.** Each lies 200–250 m from its path, mostly beside a step
  shorter than twice the radius. The line between two samples comes within the radius where no
  sample does, so these are mostly not passes of the train.

What looking for the oracle's shortfalls established, on 2026-11-07, over the 1000 sessions of run
`4245cafc-551c-510c-bddf-db9ef4221d1c` and their 12,672 passes. The run was removed on 2026-11-07,
when `evaluation_run_v2` replaced the run row's four fingerprint columns with one map.

- **The oracle falls short of a perfect score nowhere.** It falls short only on two passes of one
  crossing less than 40 s apart, where its alert for the second comes under 30 s ahead.
- **No crossing is passed twice within 90 s.** 72 passes repeat a crossing the session passed
  before, and the shortest gap between two passes of one crossing is 90 s. A pass is one visit
  within the match radius, so a train leaves the radius and comes back between two passes. 15
  repeats come within 5 minutes, and each still scores as a hit.

### Observations

- **The re-derive drops what `explore2.py` excluded, session for session** (2026-11-05).
  `just silver-motis-ingest` wrote 162,256 sessions and 62,435,542 samples. It dropped 3,342 trips
  as too fast and 8 as leaving their country, beside 7,977 already dropped as overlapping. The
  sessions written are those that `all_session_checks` keeps, with none missing and none extra.
- **34,521 written sessions reach `realtime_percentage` ≥ 90** (2026-11-05). `explore1.py` counted
  about 40,000 trips per polled day above 90%, from the share of legs rather than samples. That
  figure was an estimate, and 34,521 sessions is enough for the evaluation.
- **The re-derive takes about 11 minutes and 4.7 GB** (2026-11-05). Compiling takes 7 of the
  minutes, and deriving the segments and sessions the other 4.
- **Matching through the index of crossings takes 21–33 s and 5.0–7.8 GB** (2026-11-05), over two
  runs. `match_crossings` wrote the 283 device passes the previous matching wrote. It wrote 2,918,145
  train passes, over 143,089 of the 162,256 sessions and 4,503 of the 5,749 crossings. Ranking the
  samples by `t` takes most of the memory.
- **Deciding nearness by the tree's squared distance moves no pass** (2026-11-05). Every pass
  stays, and no distance changes by more than 2.8 × 10⁻¹⁴ m. No pass records a distance beyond the
  radius.
- **`pick` picks 1000 sessions from the store in under a second** (2026-11-06). In a debug build,
  it picked 1000 of the 34,521 sessions at or above 90 in 0.87 s. Picking all 34,521 succeeded, and
  picking one more was refused as too few. The three runs peaked at 137 MB. The two geometry
  columns hold 1,478 MB of the 1,485 MB the sessions take in their files, and a pick reads neither.
- **A change to `pick` rebuilds `evaluation-py` in 20 s** (2026-11-06). Two rebuilds through uv,
  each after a one-word change to `pick`, took 20.4 s and 19.3 s. The extension links
  `medallion-model`, DataFusion, and SedonaDB, since `pick` reads the store through medallion.
  The first build took 12.5 minutes, beside a build of `lookout-medallion` for the tests'
  fixtures.
- **Building a pick's offline detectors takes milliseconds, and reading their passes takes the
  time** (2026-11-06). The example `offline_sizing` ran in a debug build over Germany's store. It
  built the 1000 detectors of a pick in 13 ms. Reading the pick's 12,672 passes alone took 0.73 s,
  and the whole run took 4.1 s, at a peak of 46 MB. Reading all 2,918,145 passes took 29 s instead,
  and that run peaked at 1.65 GB, nearly all of it the rows read. Building a detector for each of
  the 162,256 sessions took 5.2 s. A detector holds 16 bytes per pass. `train_session_crossing_v1`
  is laid out by date alone, so `read_of_country` refuses it, and the example reads it through SQL.
- **CrowFlies' replay of 1000 sessions takes 43 s, half of it reading** (2026-11-06). A throwaway
  example replayed a pick of 1000 sessions through CrowFlies in `f32`, over Germany's 5,749
  crossings, into 55,775 transitions. In a debug build it took 140 s, at a peak of 383 MB. The same
  replay against no crossings took 20 s, so measuring every crossing at every sample takes the
  rest. Optimising `evaluation`, `predictor`, `domain`, `geo`, `geo-types`, and `num-traits` cut
  the replay to 43 s. The reads stayed at about 20 s, with DataFusion unoptimised. Optimising
  `predictor` alone changed nothing, since `CrowFlies<f32>` compiles in the crate that names it.
- **Replaying sessions in parallel cuts CrowFlies' replay to 41 s in a debug build** (2026-11-06).
  The same pick, on 8 cores, gave the same 55,775 transitions. The replay took 41 s against 140 s,
  at a peak of 425 MB against 383 MB. With the six crates optimised, it took 21 s against 43 s, and
  reading took 16 s of that. Reading now takes most of a replay.
- **The oracle scores perfectly over 1000 sessions** (2026-11-07). `just evaluate` wrote run
  `bef2e910-7b2e-57d7-b0b0-bd67cfa3f098` over 1000 sessions in Germany, 904.6 hours of them, under
  the first runs' settings. It scored 12,082 passes and 12,082 notifications, with no false alarm,
  repeat, or miss. Precision and recall are each 1, with an interval of 1 to 1, and every lead time
  is 5 minutes. A further 590 notifications credit a pass in a session's first minute, and go
  unscored. The release build took 3 min 10 s, and the run about 4 s. The run's five datasets take
  340 KB in gold. The run was removed on 2026-11-07, when a run's code came to be named by a version
  rather than a commit.
- **Reading a pick's samples takes 0.36 s in a release build** (2026-11-07). The example
  `sample_read_timing` read the 326,656 samples of a pick of 1000 sessions in Germany. Reading
  every column took 0.93 s, and switching on filter pushdown cut that to 0.50 s. Reading only the
  five columns replay uses cut it to 0.36 s. The 16 s that the task cited came from a debug build,
  with DataFusion unoptimised.
- **An R-tree of the crossings cuts CrowFlies' replay of 1000 sessions from 3.7 s to 0.54 s**
  (2026-11-07). The example `crow_flies_index_timing` replayed a pick of 1000 sessions in Germany
  through CrowFlies in `f32`, in a release build. Scanning every crossing took 3.66 s, and the
  R-tree took 0.54 s. Both gave the same 55,775 transitions. The 3 s saved did not justify the
  index, and it was removed.

### Pending refactors

- [ ] Write medallion's files through `Root::object_store()`, as a dataset's fingerprint reads
      them. The writer builds its own `LocalFileSystem` from each file's absolute path, so it alone
      ties the store to the local disk.
- [ ] Fold `evaluate`'s binary into `evaluation`, once `code_version`'s build script watches only
      tracked files. Until then the script rebuilds `evaluation`, and so `evaluation-py`, on any
      write under `crates/`. The binary's `clap` and `tracing-subscriber` go behind a feature.

### Tasks

#### Refactors / extensions

- [-] Run `just outdated`, and upgrade what it lists that this slice touches, each in a commit of
      its own. Run on 2026-11-04 and the upgrades left for an upgrades slice. SedonaDB 0.5.0 gates
      most of them, since it moves arrow to 58, datafusion to 54, and geo to 0.33.
- [x] Move the device out of `session_crossings::matching::Session`, since a train session has none.

#### Phase 1: the sessions and their passes

The evaluation replays these datasets, and their counts show whether the filter,
`realtime_percentage` ≥ 90, leaves enough sessions.

- [x] Look for anomalies in `train_session_sample_v2` with the user, in a new
      `notebooks/evaluation/explore2.py`, before v3 is declared.
- [x] Decide, from the anomalies found, which samples or sessions the cleaning rule discards.
- [x] Add `realtime` to `TrainSessionSampleRowV3` and `realtime_pct` to `TrainSessionRowV3`, written
      by `motis::train_sessions` in place of v2. The v2 row types became v3 in place, as a silver
      version does, so their snapshots went too. The v2 row types later came back, read and no
      longer derived, so that `explore2.py` can still read v2. The column became
      `realtime_percentage`, a `Percentage` that refuses a value outside 0–100.
- [x] Apply the cleaning rule's two checks in `motis::train_sessions`, as drop reasons counted
      beside its existing four. The country check tests each leg's line, once per polyline, against
      the country's land and maritime areas joined into one. A sample lies on its leg's line, so
      this agrees with a test of every sample, at a fraction of the cost.
- [x] Re-derive with `just silver-motis-ingest`. Check that about 40,000 sessions reach
      `realtime_percentage` ≥ 90, as `explore1.py` counted per day and trip. 34,521 reach it, and
      the 40,000 was an estimate.
- [x] Remove `TrainSessionRowV2`, `TrainSessionSampleRowV2`, and their snapshots, and delete the v2
      files from the local store. `explore1.py` and `explore2.py` stay as the record of what they
      found, and their cells that read v2 no longer run.
- [x] Add `TrainSessionCrossingRowV1`, and derive `train_session_crossing_v1` in `session_crossings`
      through the matching `session_crossing_v3` uses.
- [-] If reading every train sample at once runs out of memory, match one date at a time. Moot:
      one date at a time leaves the 16.8 billion distances, and splits a session that runs past
      midnight. The index of crossings below fixes both the memory and the time.
- [x] Match each sample against an R-tree of the crossings, with the samples streamed and ranked by
      `t` within their session. A visit is then a run of consecutive ranks near one crossing. Both
      kinds of session match this way.
- [x] Rank samples with equal `t` by a property of the data, so that every run ranks them alike.
      They rank by easting and then by northing. Neither sample dataset holds two samples of one
      session at the same `t` today.
- [x] Measure in `explore2.py` the spacing of samples in the sessions the filter keeps, against the
      250 m match radius. Measured in `explore2_spacing.py` instead, since the cells of
      `explore2.py` that read v2 no longer run. The spacing loses about 0.1% of passes, which is
      acceptable, so interpolating a pass along the path stays deferred.

#### Phase 2: the skeleton, run with the oracle

The oracle's score is known before it runs: perfect, short of what the data or the rules forbid. A
run of it tests every new part, from the tracker to gold, before a real predictor sits behind them.

- [x] Add a type per state to `predictor`, a consuming method per legal transition, and
      `CrossingState` over the types.
- [x] Add `Tracker` to `predictor`, holding each crossing's `CrossingState` and recording each
      `Transition`, tested on constructed judgements. It takes predictions with a confidence and the
      crossings passed, as the decisions on confidence and the pass detector set out.
- [x] Create the `evaluation` crate with the step `pick`, which picks `TrainSessionRowV3`s at
      random under a seed. It reads one country through medallion's new `Read<T>`, and the engine
      filters and picks, as the decisions on the typed read and the pick set out.
- [x] Add the `evaluation-py` extension exposing `pick`, and time a rebuild after a change to
      `pick`.
- [x] Decide from that time whether the steps take and build `medallion-model`'s rows, or rows of
      their own.
- [x] Add the trait `DetectPasses` to `predictor`, and its offline implementation to `evaluation`,
      which reports a session's passes from `train_session_crossing_v1`.
- [x] Add the step `replay` to `evaluation`, feeding a session's times and positions alone through a
      predictor, the offline pass detector, and the `Tracker`, in the float type its run records.
- [x] Add the oracle to `evaluation`: a predictor that reads a session's ground-truth passes, and
      predicts each with high confidence from N ahead.
- [x] Add the step `score` to `evaluation`, scoring each notification as
      [The measure](#the-measure) defines.
- [x] Leave each session's first minute out of `score`, as the decision on `ActualPass` sets out.
      The minute is `Scoring`'s `warm_up`, counted from `started_at`. A notification that repeats an
      unscored pass goes unscored too.
- [x] Add the step `summarise` to `evaluation`: the measures and their intervals, with the counts
      of sessions, passes, and notifications.
- [x] Expose every step and the full run in `evaluation-py`, with geometry crossing as GeoArrow.
      No step's table holds geometry yet, so none crosses as GeoArrow.
- [x] Build a new `notebooks/evaluation/explore3.py` with the user, running the oracle's steps on a
      few sessions and drawing each.
- [x] Extend `medallion` to gold datasets of rows, partitioned `run_date=<date>/run_id=<id>`, each run
      written once.
- [x] Register gold datasets by name in `medallion`'s `Query`, and add `query_gold` and `write_gold`
      to `medallion-py`.
- [x] Define the gold datasets that [The datasets](#the-datasets) names, in `medallion-model`.
- [x] Add the `evaluate` crate, whose CLI runs every step in order or any one alone, and a
      `just evaluate` recipe.
- [x] Run the oracle over 1000 sessions with `just evaluate`.
- [x] Check that a run recorded in `explore3.py` with the CLI's settings is the CLI's run: the same
      run id, rows equal to those `query_gold` reads back, and `write_gold` refusing it as already
      written. A run's id hashes its settings, code, and data, so the notebook cannot write a
      second copy.
- [x] Trace each shortfall from a perfect score in `explore3.py`, through `query_gold`, and record
      its cause under Investigation. There was none to trace, and Investigation records why.
- [x] Check `summarise`'s own bootstrap against a proven statistics library's, as the decision on
      `summarise`'s bootstrap sets out. Replacing it was the task as first written.
- [x] Do a `/simplify` pass over all code in this branch, and expand into different refactors under this todo entry
      Run on 2026-11-07 over the crates, the `Justfile`, and `Cargo.toml`, without the notebooks.
      Four reviews, for reuse, simplification, efficiency, and altitude, found the refactors that the
      next task lists. Three of their suggestions conflict with a decision or a later phase, and are
      left out: replacing the typestate structs with `StateKind`, removing the `RecordedPass` trait,
      and removing `PassDetectorName::Online`.
- [ ] Apply the refactors the `/simplify` pass found.
      A refactor that changes a crate the M5 compiles, `predictor`, `domain`, or `platform-core`, is
      checked on the device as well as by the tests. In `predictor`, those are CrowFlies' crossings
      query and the rule for the order of events. In `domain`, they are the moved enum, the `Span`
      conversion, and the newtypes. Making `crossings::compacted` generic touches the point set the
      device carries, so the device checks it too. `just m5plus-build` builds the firmware. The user
      then flashes it with `just m5plus-flash` and reads its console, since Claude cannot open the
      serial port. A dependency one of these crates gains follows
      [the device's rule on dependencies](device.md#a-device-crate-spells-out-what-the-workspace-leaves-on).
      - **Evaluation pipeline**
        - [x] Narrow replay's read of samples to `session_id`, `t`, `seq`, `lat`, and `lon`, and
              turn on parquet filter pushdown. Reading takes 16 s of a 21 s replay, since all of a
              country's 62M samples are decoded to keep about 300k. The 16 s came from a debug
              build, as Observations records.
        - [-] Give `Crossings` a `near(from, radius)` query, which the device keeps as a scan and
              the evaluation backs with one shared R-tree. CrowFlies measures about 1.7 billion
              distances in a run of 1000 sessions, and each radius in Phase 3's sweep measures them
              again. Moot: in a release build, the scan costs about 3 s per 1000 sessions, as
              Observations records. That is too little to justify an index the device does not
              share. The index comes back once the device has more to do with each fix. It is then
              built once, for the device and the evaluation alike.
        - [x] Move writing a run and resuming one from `evaluate` into `evaluation`, and keep each
              step's rows in memory through `evaluate run`. A notebook cannot write a run as the CLI
              does, and a run fingerprints silver four times and reads back what it wrote. A
              notebook writes a run through `evaluation-py`'s new `evaluate`.
        - [x] Read a run's crossings and passes once, and resolve each pass's compact id once. An
              oracle run reads the passes three times and the crossings twice.
        - [x] Add `Query::read_of_run::<T>` and `medallion::from_batches`, and drop the SQL text
              that `read_run_rows` builds. The loop that decodes batches is written five times, and
              the SQL text breaks the decision on typed reads. Done without `read_of_run`, which
              ties `Query` to runs. `Query::read` exposes a dataset's partition levels as columns,
              and a dataset declares the levels beneath its key with `partitioned_within`.
        - [x] Write gold through a target beside `SilverTarget`, with its column checks and a real
              `TableWritten`. Gold accepts unexpected columns, and `write_gold` makes up the counts
              it returns.
        - [x] Give each mirrored enum one home, such as `StateKind` in `domain`, and parse the float
              type and the predictor with `strum`. Four enums name the float type, each pair joined
              by a converter.
        - [x] Convert between a run and its row with `From` and `TryFrom`, give `Span` a
              `TryFrom<TimeDelta>`, and give `Score` a `FromStr`. `span_of` writes milliseconds as
              text to parse them back, and the CLI wraps parsers that exist. A row is still built
              by `run_row`, since it takes the run's inputs and start as well as the run.
        - [x] Define a run's fingerprinted inputs once, with a `changed_since`, and read their
              fingerprints concurrently. The four datasets are listed in four places across two
              crates. The run row became `evaluation_run_v2`, whose fingerprints are one map
              keyed by dataset, so the datasets are listed once, in `evaluation`.
        - [x] Have the steps take each session's id, start, and end, rather than silver's
              `TrainSessionRowV3`. That drops `read_sessions`, a second `IN` filter, and a second
              formula for a session's hours.
        - [x] Give predictors, pass detectors, and the tracker one rule for the order of events,
              such as a `Clock`. Four copies hold the rule, with two error types, and two of the
              copies already differ. The rule is CrowFlies' own: a fix is ordered against the last
              fix, and a tick against the clock. The oracle and the offline detector now accept a
              fix that lags a tick, which a replay never sends.
        - [x] Make `crossings::compacted` generic over the float type, and call it from the
              evaluation. The evaluation holds a second copy of how a device compacts its crossings.
              Checked against the device by repacking: the point set came out byte for byte the
              same, and the firmware does not link `crossings`.
        - [x] Share the pyo3 helpers of the three extensions, with one mapping of errors to Python
              exceptions, and classify `StepError` once. One store error raises `ValueError` in one
              module and `RuntimeError` in another.
        - [x] Import `evaluation`'s test helpers from `fixtures`, and let `write_geography_rows`
              write a dataset partitioned by country alone. `instant()` is defined about ten times,
              and three crates build a crossings table by hand. The write by country is its own
              function, `write_geography_rows_by_country`, since the dated one groups rows by date.
      - **Build identity**
        - [x] Take `motis`'s build identity from `code_version`, move `CodeVersion` into that crate,
              and drop `BUILD_GIT_HASH` from the recipes. The two schemes test for uncommitted
              changes differently, and a plain `cargo run` of `motis` records "unknown". The
              server keeps `BUILD_GIT_HASH`, since it builds in Docker, where no repo is there to
              read a version from.
        - [ ] Watch only tracked files in `code_version`'s build script. It watches all of
              `crates/`, so a write under a `.venv` or `m5plus/target` relinks `evaluate`.
      - **Derivation pipeline**
        - [ ] Read the capture and place its polylines once per `motis_ingest`, and pass both to
              both derivations. A re-derive reads, deduplicates, and places the whole capture twice.
        - [ ] Build capture's `Coverage` from the prepared outline of `CountryAreas`, and memoise
              `keeps` by polyline. The two unions of a country disagree on a shape that is not a
              polygon, and most of a poll's relates repeat an earlier answer.
        - [ ] Rank train samples by their stored `seq`, rather than by a `ROW_NUMBER()` window. The
              window takes 6.3 GB, most of the matching's peak.
        - [ ] Project each train sample once, close each day's samples once they are written, and
              stop holding every session's path. All three feed the 4.7 GB peak of a re-derive.
        - [ ] Find passes one way, by building `steps` on `Matcher`, and point the geo test at that
              path. The store-backed test checks a path that production never runs.
        - [ ] Flatten `TrainSegmentRowV4` into `Leg`, and make `Trip.country` a `Country`. `Leg`
              copies eleven fields by hand, and the `Option` exists only to count a drop.
        - [ ] Give the division-area path one helper, and list landed files through the object
              store. Three places spell the path, and a second walk of the disk lists the files.
        - [ ] Declare every validated newtype with `nutype`. Two idioms coexist, and each
              hand-written type takes 25 to 60 lines.
        - [ ] Describe the table of `session-crossings-py` with medallion's fields. Its copy has
              drifted, and leaves `crossed_at` non-nullable.

#### Phase 3: CrowFlies through the skeleton

CrowFlies is the baseline that later predictors are measured against, and the first predictor the
tracker meets that can be wrong.

- [ ] Give each of `CrowFlies`' predictions a confidence: low within its radius, and high while the
      crossing arrives within N.
- [ ] Run CrowFlies over the oracle's sessions and seed with `just evaluate`.
- [ ] Sweep the radius. Choose CrowFlies' settings from recall
      plotted against false alarms per hour.
- [ ] Draw CrowFlies' false alarms and misses on a map in a new `explore4.py` with the user, and one
      session's transitions over time.

#### Phase 4: LightCone

LightCone is the improvement the Target asks for, measured against CrowFlies on the same sessions.

- [ ] Measure in a new `explore5.py` the angle between a train's heading and its positions up to N
      later.
- [ ] Read ballpark odds for the extremes of that angle, and bound the cone's angle by them.
- [ ] Add `LightCone` to `predictor`: a velocity from the last two samples, and high confidence
      inside the cone it reaches within N.
- [ ] Run LightCone over the same sessions and seed, with angles up to that bound, and compare its
      best settings with CrowFlies'.
- [ ] Average the velocity over the last few samples, as a setting of `LightCone`, and run it again.
- [ ] Draw LightCone's cone at each sample in `explore5.py` with the user, where it beats
      CrowFlies and where it loses.
- [ ] Run both predictors over every session the filter keeps. If their intervals still overlap,
      widen the filter to every session.

#### Phase 5: debouncing, an optional experiment

A run shows whether debouncing a predictor's predictions improves its precision and recall. The apps
can take a predictor without it, so this phase can be skipped.

- [ ] Debounce `LightCone`'s predictions and `CrowFlies`', each as a setting of its predictor.
- [ ] Run each predictor with and without its debouncing, over the same sessions and seed, and
      compare precision, recall, and false alarms per hour.

#### Phase 6: the online pass detector

The apps cannot read ground truth, so they detect each pass from the samples alone.

- [ ] Add the online implementation of `DetectPasses` to `predictor`: a crossing within a pass
      radius of the latest sample.
- [ ] Evaluate the online detector against the offline one over the same sessions and seed,
      sweeping the pass radius, and choose the radius with the user.

#### Phase 7: states in the apps

The apps come last, since they show the states of the predictor a person chooses from the runs.

- [ ] Write the skill `choose-predictor` in the repo's `.claude/skills/`, which reports the runs
      in gold as a dated doc in `docs/`.
- [ ] Choose the apps' predictor and settings with the user, through `/choose-predictor` over the
      runs of Phases 4 and 5.
- [ ] Hold the `Tracker` in `platform-core`'s model, around the chosen predictor and settings and
      the online pass detector, and expose each crossing's state.
- [ ] Carry each crossing's state in `web-core`'s `Predicted`, and draw one shape per state in
      `lookout-predictor.js`.
- [ ] Prefix each crossing's line in `m5-core`'s `panel.rs` with one character per state, within 13
      characters.
- [ ] Build with `just wasm` and `just m5plus-build`. Hand over `just m5plus-flash`, and a look at
      `/kiosk`.

#### Wrap-up

- [ ] Fold what held of `2026-08-01-evaluation.md` into `docs/evaluation.md`, repoint the links to
      it, and delete it.
- [ ] Add `train_session_crossing_v1` and the gold datasets to the derivation graph in
      `docs/architecture.md`.
- [ ] Describe gold datasets of rows, and writing them from Python, in the gold sections of
      `docs/medallion.md`.
- [ ] Describe `query_gold` and `write_gold` in `crates/medallion-py/README.md`.
- [ ] Describe `realtime` and `realtime_percentage` in `crates/motis/README.md`, and train sessions
      in `crates/session_crossings/README.md`.
- [ ] Rewrite `predictor`'s README for the typed states, the tracker, the pass detectors, and
      LightCone, and `platform-core`'s for the states it exposes.
- [ ] Show the states in the picture in `docs/web.md`, and on the panel in
      `crates/platform/m5/README.md`.
- [ ] Write the READMEs of `evaluation`, `evaluate`, and `evaluation-py`.
- [ ] Name the v3 datasets in the help of `silver-motis-ingest`, and `train_session_crossing_v1` in
      that of `silver-session-crossings`.
- [ ] Fold the `exploreN.py` notebooks into one or more notebooks that demonstrate the evaluation's
      behaviours or steps. Then remove them, with their answers in the removing commit.
- [ ] Run `just test-no-docker`, and hand over `just test`.
