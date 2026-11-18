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

## Ground truth

A pass is one visit of a session to a crossing: a run of consecutive samples within a match
radius of the crossing. A session that reverses over a crossing passes it twice. The visit's
nearest sample gives the pass its instant.

A crossing on a track parallel to the one travelled can lie within the match radius. Such a
crossing is recorded as passed.

## The unit of counting

The unit is the pass, one (session, crossing, pass) triple. Each notification and each pass
contributes at most one outcome, however many samples the approach held.

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

## Not covered

These lie outside the evaluation. Each is cheaper to add later than to remove:

- **Calibration.** A prediction carries a confidence of low or high, and no measure checks that
  the confidence matches how often a prediction is right.
- **Error in the estimate of arrival.** The evaluation scores notifications, not the estimated
  time to a crossing, so no measure reports the estimate's error by remaining time.
- **Parallel lines.** A pass recorded for a crossing on a parallel track stays in the ground
  truth until sessions are matched to track.
- **The cause of a miss.** No measure separates a predictor's error from a train's unforeseeable
  behaviour, such as an unscheduled stop.

## Approaches considered and rejected

**Counting per sample**: at each sample, whether the predictor judged each crossing within reach
correctly. The evaluation counts per pass instead, for two reasons:

- **Slow approaches dominate.** With a sample every 10 seconds, an approach lasting 10 minutes
  counts 60 times, and one lasting 1 minute counts 6 times. Each is still one approach. An
  average over samples is therefore mostly an average over slow approaches and densely sampled
  sessions.
- **The predictor sets its own denominator.** The number of samples counted depends on how long
  the predictor keeps a crossing within reach. A predictor could then improve its score by
  changing what it counts, rather than by predicting better.

**Precision and recall per prediction cycle, over a space of crossings and instants.** The imbalance
between passes and the space of every crossing at every instant makes precision and recall the
right family, over accuracy or ROC. Three definitions make the numbers unresponsive to the
predictor:

- Recall defined as whether any prediction was made is satisfied by a predictor that predicts every
  crossing within its search radius. It then measures the search radius against the match radius.
- Precision conditioned on a crossing's pass cannot score a prediction for a crossing the train
  never reaches.
- Counting each cycle is counting per sample, so it has the two problems that counting per sample
  has: slow approaches dominate, and the predictor sets its own denominator.

**Prognostics metrics**, such as α-λ accuracy, prognostic horizon, and convergence. The domain is
similar: repeated estimates of the time to an event, refined as the event approaches. A tolerance
in proportion to the time remaining is attractive. These metrics fail here on three grounds:

- They assume prediction gets easier as the event nears. A train can stop without warning close
  to a crossing, so the tolerance is tightest exactly where the error is least reducible.
- Prognostic horizon requires an estimate to enter a band and stay inside it. One late disturbance
  then discards an otherwise good approach.
- They assume the event happens. A crossing that the train never reaches is a principal failure
  here, and they have no place for it.


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
