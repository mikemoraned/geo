# Evaluating a predictor

This doc covers how a predictor of upcoming water crossings is measured against recorded train
sessions. It is for whoever runs an evaluation, reads its results, or changes how it scores.
Crossing means a water crossing throughout.

The product raises one notification for each crossing a train approaches. A notification is
useful when it arrives early enough to act on, and not so early that nothing is yet visible. The
evaluation therefore scores the notification, not the predictor's estimate of when the train
arrives.

## What a run does

A run picks sessions, replays each through the predictor, scores the notifications against the
recorded passes, and sums the scores into measures:

1. **Pick.** A seeded draw of sessions from a country's recorded train sessions, each with a
   share of realtime positions at or above a threshold.
2. **Replay.** Each session's samples drive the predictor and the tracker. The tracker holds each
   crossing in one of four states: out of reach, candidate pass, alerted pass, and actual pass.
   The replay records every transition between states.
3. **Score.** Each entry into the alerted state is a notification. Each notification is scored
   against the recorded passes of its session.
4. **Summarise.** The scores sum into measures, each with an interval.

A run records its settings, the code it ran, and a fingerprint of each input dataset. The same
settings over the same code and data give the same run.

### What each choice guards against

A run makes four choices before it reports a number. Three of them each guard against one way the
number can mislead. The pick guards against nothing, and trades precision for speed:

1. **The filter** guards against scoring the predictor against the plan rather than the train. A
   session's positions are both the predictor's input and the ground truth. A timetable's
   positions show where the train was planned to be, and realtime positions show where it was. The
   filter keeps the sessions whose share of realtime positions reaches a threshold.
2. **The pick** sets how many of the kept sessions a run replays, under a seed, or replays every
   one. Fewer sessions give a quicker run with wider intervals, and the seed keeps a small sample
   the same from run to run. A run over every session loses nothing to the pick.
3. **The split** guards against reporting a number on the sessions used to choose the settings.
   It assigns each place to the tune set or the test set, and a run counts only what happens in one
   set's places. Every session replays in full, so the predictor keeps its history across places
   that do not count. [Tuning and testing](#tuning-and-testing) sets out the split.
4. **The bootstrap** guards against reading noise as a difference. It redraws the counted sessions
   to estimate how far each measure would move under another draw of sessions like these.
   [Intervals](#intervals) sets out the bootstrap.

## Ground truth

A pass is one visit of a session to a crossing: a run of consecutive samples within a match
radius of the crossing. A session that reverses over a crossing passes it twice. The visit's
nearest sample gives the pass its instant.

A crossing on a track parallel to the one travelled can lie within the match radius. Such a
crossing is recorded as passed.

## The unit of counting

The unit is the pass, one (session, crossing, pass) triple. Each notification and each pass
contributes at most one outcome, however many samples the approach held.

A **scored unit** is anything that takes an outcome: a notification, which is credited, a repeat,
or a false alarm, and a pass that no notification credits, which is a miss. Each scored unit
belongs to one crossing.

## Scoring a notification

A notification is measured to the next pass of its crossing, at or after the notification. Its
lead is the time from the notification to that pass. A horizon N and a shorter last moment L divide
the leads into three classes, each earning a credit from 0 to 1:

| Class | Notified | Credit |
| --- | --- | --- |
| Too early | more than N before the pass | falls along a ramp from 1 to a floor, then stays at the floor |
| Hit | from N to L before the pass | 1 |
| Last moment | less than L before the pass | a fixed partial credit |

The ramp past N makes the credit continuous rather than a step. A notification slightly too early
earns nearly full credit.

Each notification takes one verdict:

- **Credited.** The notification earns its credit. A pass credits one notification: the one that
  scores worst among those measured to it, and of those that score alike, the earliest.
- **Repeat.** Another notification measured to the same pass earned its credit. A repeat scores
  0.
- **False alarm.** No pass of the crossing follows the notification. A false alarm scores 0.
- **Unscored.** The pass falls within the warm-up at the start of its session, and the
  notification counts in no measure.

A pass that credits no notification is a miss, unless it falls within the warm-up. A predictor
needs samples before it can predict, so a pass in the warm-up is neither scored nor missed.

A run records its horizon, last moment, last-moment credit, ramp, floor, and warm-up.

### Examples

These examples take a horizon N of 5 minutes, a last moment L of 30 seconds, a last-moment credit
of 0.25, a ramp of 1 minute, and a floor of 0.05. Each row is one notification, or one pass with
none:

| Case | Class | Verdict | Score |
| --- | --- | --- | --- |
| Notified 2 minutes before the pass | Hit | Credited | 1 |
| Notified 10 seconds before the pass | Last moment | Credited | 0.25 |
| Notified 5 minutes 30 seconds before the pass, half way down the ramp | Too early | Credited | 0.525 |
| Notified 8 minutes before the pass, past the end of the ramp | Too early | Credited | 0.05 |
| Notified, and the crossing is never passed | – | False alarm | 0 |
| Passed, with no notification before it | – | Miss | 0 |

A pass with more than one notification credits the one that scores worst. Take a crossing notified
7 minutes before its pass, withdrawn, and notified again 2 minutes before it:

| Notification | Class | Verdict | Score |
| --- | --- | --- | --- |
| 7 minutes before the pass | Too early | Credited | 0.05 |
| 2 minutes before the pass | Hit | Repeat | 0 |

The pass earns 0.05, not 1. Notifying again in time does not recover the credit that an early
notification lost.

## Measures

- **Precision.** The credit earned, divided by the notifications raised.
- **Recall.** The credit earned, divided by the passes.
- **False alarms per hour.** False alarms, divided by the hours of session scored. This is the
  measure closest to what the notification costs in use.
- **Lead.** The 10th, 50th, and 90th percentiles of the lead, over the notifications that passes
  credit.
- **F0.5.** The weighted harmonic mean of precision and recall, with β = 0.5. A shortfall in
  precision lowers it more than the same shortfall in recall. The product prefers missing a
  crossing to notifying about one the train does not reach.

The measures follow forecast verification, the field that scores weather forecasts. It counts
yes/no forecasts in a table of hits, misses, false alarms, and correct negatives. For rare events it
leaves the correct negatives out, as the [critical success index](https://glossary.ametsoc.org/wiki/POD,_FAR,_and_CSI) does. The evaluation does the same:
it counts credited notifications, misses, and false alarms, and never counts a crossing correctly
left without a notification. Every crossing at every instant would be such a negative, and their
count would swamp every measure. Forecast verification also suits the problem: a forecast is
revised as its event approaches, covers many places at once, and its event need not happen.

Precision and recall share the credit earned as their numerator, so a notification credited in
part counts in part towards both.

## Intervals

Each measure carries a 95% interval, from a bootstrap over sessions. A resample draws sessions
with replacement, under a seed, and the interval spans the 2.5th to the 97.5th percentile of the
measure across resamples. Resampling sessions rather than notifications keeps the notifications of
one session together, since they are not independent.

A result is reported with the number of sessions and passes it covers, and the interval around
each measure. At small samples, an interval spans most of the plausible range.

## Choosing settings

A predictor's settings are swept rather than fixed in advance. Each setting gives a run, and the
runs are compared by F0.5, with precision, recall, and false alarms per hour beside it. A choice is
recorded with the runs that support it.

A result needs a baseline to compare it with. The oracle is one bound: a predictor that alerts each
recorded pass the horizon before it. It scores perfectly except where two passes of one crossing
fall within the horizon of each other. A run of the oracle therefore checks the scoring itself.

## Online and offline passes

The scoring takes its passes from the recorded ground truth, an offline pass detector that sees
the whole session. A device sees only the samples so far, so it detects passes online, from a pass
radius around each crossing.

The evaluation scores a predictor offline, and the apps run it online. If the two detectors
disagree, a predictor can behave differently in the apps from how it scored. The difference then
comes from how passes are detected, not from the predictor. Measuring the skew between the
detectors separates the two causes. A small skew means a run's scores carry over to the apps. A
large or systematic skew, such as online passes that arrive consistently late, is a fault in the
online detector to fix before the scores are trusted.

A run with an online detector measures how far its passes skew from the offline ones:

- **Seen.** The share of offline passes that an online visit matches.
- **Unmatched.** The share of online visits that match no offline pass.
- **Offset.** The time from each offline pass to the online visit matching it, as percentiles.

## Tuning and testing

**Tuning** is anything that sets a predictor's settings from sessions. It takes two forms:

- **Measuring a setting from the data**, such as the largest angle trains turn through within N.
  Such a measure reads **windows**: a window is one session's samples from a given sample to N
  later, and the measure takes one value from each window, such as the angle the train turns
  through.
- **Choosing settings from a sweep.** Several runs replay the sessions under different settings,
  and the best score picks one.

A predictor tested on the sessions it was tuned on scores better than it will on sessions it has
not seen. The best of several runs also scores high on those runs partly by chance.
[Cawley and Talbot](https://jmlr.org/papers/v11/cawley10a.html) show this selection bias to be as
large as the differences between the methods they compare. The evaluation therefore keeps a
**tune** set, which tuning and every sweep read, apart from a **test** set, which each predictor's
chosen settings run over once.

Two sets suffice while tuning chooses among a few settings. If a predictor learns many numbers from
the data, as a model's weights are learned, it needs a third set to choose its settings on. The
predictors the evaluation runs learn no such numbers, so it keeps two sets.

A test set that informs a choice becomes a tune set, and each comparison on it is then a sweep.
A fresh test set, such as a later capture, removes that bias.

### Why the split is by place

A session is one trip on one day, and sessions repeat each other in three ways:

- **One trip on several days**, over the same track at the same times.
- **One line through the day.** A line with a train every 20 minutes gives dozens of sessions a
  day over the same track.
- **One corridor, several lines**, where different trains share track between two cities.

How far a train turns within N is a property of the track. A random split of sessions can put the
10:00 train of a line in one set and the 10:20 train in the other. It then tunes and tests on
the same curves. [Roberts et al.](https://doi.org/10.1111/ecog.02881) show that a random split of
data with this structure overstates how well a model does on new data. Their remedy is to split by
group, or by block of space or time, so that each group falls wholly on one side.

The split is therefore by cell of a global grid.
[Splitting by trip or by line](#splitting-by-trip-or-by-line) sets out why neither serves as the
group. Cells at one resolution cover the globe without overlap, so each crossing and each sample
lies in exactly one cell. The split needs nothing from a feed but positions, and works the same in
every country.

### Cells and their sets

The grid is [A5](https://a5geo.org/), which tiles the globe in pentagons of equal area at each
resolution. A cell then covers the same ground in every country, where the cells of
[H3](https://a5geo.org/examples/area) vary in area by a factor of about 2. The resolution is 8,
whose cells cover 519 km², about 23 km across. A finer resolution cuts more of the
[windows](#tuning-and-testing) tuning reads across two sets. A coarser one puts more sessions in
each cell, so the sets come out more uneven.

A cell's set comes from a hash. The SHA-256 of a salt and the cell's id, modulo 100, gives the
cell a bucket. A cell whose bucket falls below the tune buckets goes to tune, and the rest to test.
The assignment depends on the hash alone, so it holds across reruns and library upgrades, as
repeatable splitting in
[*Machine Learning Design Patterns*](https://www.oreilly.com/library/view/machine-learning-design/9781098115777/)
sets out. A run records its resolution, salt, and tune buckets. The salt is fixed for good once a
predictor has read the tune set: a change puts track the predictor was tuned on into its test set.

### Scoring by each crossing's cell

A run replays each session whole, and scores each unit in the set of its crossing's cell. A
notification, a pass, a false alarm, and a miss each belong to one crossing, and so to one set. A
run without a split scores every unit.

This holds out what a predictor is scored on, and not what it reads as it predicts. Forecasting
does the same in
[evaluation on a rolling forecasting origin](https://otexts.com/fpp3/tscv.html), which forecasts
each target from all the history before it, and scores only the test period. So does spatial
cross-validation, which assigns each observation to a block by its location.

[Cutting sessions at cells' edges](#cutting-sessions-at-cells-edges) sets out why sessions
replay whole.

The other measures follow the split:

- False alarms per hour divide by the hours in the split's cells. Each step between two samples
  counts towards the split of its first sample's cell.
- The skew between pass detectors counts only passes and visits whose crossing lies in the split's
  cells.

Tuning reads only [windows](#tuning-and-testing) that lie wholly in tune cells, so no tuning
window reads test track.

### What the split leaves unequal

The tune and test sets match in the ordinary shape of their track: how straight it runs, and how
far a train turns within N. They differ in the rare large turns. A turn of well over 90° within N
is a reversal, as at a terminus, and a few cells hold most of them. Whichever set those cells fall
in has more of them. A setting bounded by the tune set's largest turns therefore does not bound the
test set's. Such extremes serve only to bound the range of a sweep, and the sweep over the tune set
chooses the setting.

The sets also differ in size. A busy cell moves many sessions at once, so a set's share of
sessions differs from its share of cells.

### Checks of a split

A broken split can look healthy. If a bug gives samples the wrong set, the sets can still hold
about 70% and 30% of the sessions, as intended, so looking at their sizes shows nothing wrong. Each
check below holds for every correct split, and catches errors of that kind:

1. **Disjoint.** No cell lies in both sets.
2. **Complete.** Every [scored unit](#the-unit-of-counting) of a run falls in exactly one set,
   and the units of the two sets sum to the units of the run.
3. **One cell per crossing.** Every [scored unit](#the-unit-of-counting) of a crossing takes the
   set of the crossing's cell.
4. **One set per tuning window.** A [window](#tuning-and-testing) that tuning reads lies wholly
   in tune cells.
5. **Within the country.** Every cell that holds a crossing or a sample lies among the cells that
   overlap the country's land and maritime areas. A covering of those areas finds the cells, and it
   shares no code with placing a point in a cell.
6. **Stable.** One salt, resolution, and cell give one set on every run.

Property tests over generated sessions hold checks 1 to 4 and 6. Every run with a split checks 1,
2, 3, and 5 against the store, and every tuning step checks 4. A run that fails a check is refused,
and a run that passes records the counts each check compared.

A crossing outside the country's cover is left out of a run with a split.

## Not covered

These lie outside the evaluation:

- **Calibration.** A prediction carries a confidence of low or high, and no measure checks that
  the confidence matches how often a prediction is right.
- **Error in the estimate of arrival.** The evaluation scores notifications, not the estimated
  time to a crossing, so no measure reports the estimate's error by remaining time.
- **Parallel lines.** A pass recorded for a crossing on a parallel track stays in the ground
  truth until sessions are matched to track.
- **A predictor that learns as it replays.** A split by place holds out the units a predictor is
  scored on, and not what it reads. A predictor that learns from what it reads would then learn
  from the test set.
- **A predictor that holds its data**, such as a map from where and how fast a train goes to the
  crossings it reaches. It claims to predict later trains over track it has seen, so a split by
  place tests it only where it has seen nothing. A split by time tests its claim: build on earlier
  journeys, and test on later ones.
- **The cause of a miss.** No measure separates a predictor's error from a train's unforeseeable
  behaviour, such as an unscheduled stop.

## Approaches considered and rejected

### Counting per sample

Counting per sample scores, at each sample, whether the predictor judged each crossing within
reach correctly. The evaluation counts per pass instead, for two reasons:

- **Slow approaches dominate.** With a sample every 10 seconds, an approach lasting 10 minutes
  counts 60 times, and one lasting 1 minute counts 6 times. Each is still one approach. An
  average over samples is therefore mostly an average over slow approaches and densely sampled
  sessions.
- **The predictor sets its own denominator.** The number of samples counted depends on how long
  the predictor keeps a crossing within reach. A predictor can then improve its score by
  changing what it counts, rather than by predicting better.

### Precision and recall per prediction cycle

This approach counts precision and recall over a space of crossings and instants. The imbalance
between passes and the space of every crossing at every instant makes precision and recall the
right family, over accuracy or ROC. Three definitions make the numbers unresponsive to the
predictor:

- Recall defined as whether any prediction was made is satisfied by a predictor that predicts every
  crossing within its search radius. It then measures the search radius against the match radius.
- Precision conditioned on a crossing's pass cannot score a prediction for a crossing the train
  never reaches.
- Counting each cycle is counting per sample, so it has the two problems that counting per sample
  has: slow approaches dominate, and the predictor sets its own denominator.

### Prognostics metrics

Prognostics metrics include α-λ accuracy, prognostic horizon, and convergence. The domain is
similar: repeated estimates of the time to an event, refined as the event approaches. A tolerance
in proportion to the time remaining is attractive. These metrics fail here on three grounds:

- They assume prediction gets easier as the event nears. A train can stop without warning close
  to a crossing, so the tolerance is tightest exactly where the error is least reducible.
- Prognostic horizon requires an estimate to enter a band and stay inside it. One late disturbance
  then discards an otherwise good approach.
- They assume the event happens. A crossing that the train never reaches is a principal failure
  here, and they have no place for it.

### Splitting by trip or by line

Neither a trip nor a line serves as the group of a split. A trip keeps one trip's days together,
and leaves the trains of one line apart. A line's name in the German feed names a train rather
than a line, repeats across operators, and depends on how each country's feed names its lines.

### Cutting sessions at cells' edges

A split can instead cut each session where it crosses from one cell into another, and replay
each part of a session alone, in the set of its cell. That changes the task, since a real train
meets no cell's edge. A part starts mid-approach, without the samples a predictor needs to measure
a speed. A notification near a part's end has its pass in the next part, perhaps in the other set,
so it counts as a false alarm. Over 1,000 sessions, cutting lowers CrowFlies' precision at 1 km
from 0.328 to 0.299, and the two intervals do not overlap. Scoring by cell reproduces the precision of the same predictor over whole sessions.

## References

- [POD, FAR and CSI](https://glossary.ametsoc.org/wiki/POD,_FAR,_and_CSI) and
  [WMO verification guidance](https://old.wmo.int/aemp/sites/default/files/MET_capability_demonstration_appa_verification.pdf)
  — contingency-table verification stratified by lead time
- [49 CFR 273.5 — On-time performance and train delays](https://www.ecfr.gov/current/title-49/subtitle-B/chapter-II/part-273/section-273.5)
  — punctuality windows, which treat early and late as distinct failures
- [A comprehensive review and evaluation framework for data-driven prognostics](https://www.sciencedirect.com/science/article/pii/S0888327025007162)
  — the prognostics metrics, and the assumptions they state
- [F-score](https://en.wikipedia.org/wiki/F-score#F%CE%B2_score) — the Fβ score, and how β weighs
  recall against precision
- [Bootstrapping (statistics)](https://en.wikipedia.org/wiki/Bootstrapping_(statistics)) — the
  percentile interval from resamples
