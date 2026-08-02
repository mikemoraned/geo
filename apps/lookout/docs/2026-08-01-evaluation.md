# Evaluating a predictor

*Assessment of 2026-08-01, written before any predictor existed and before any ground truth
had been derived. It records the reasoning behind a measurement design, not a measurement.
Whatever survives contact with a first run belongs in a durable doc; the rest goes stale.*

How a predictor of upcoming points of interest is measured against recorded sessions.

The product emits one notification per approaching point of interest, intended to arrive
with enough warning to act on and not so early that nothing is yet visible. The evaluation
therefore measures the notification, not the underlying time estimate: the estimate is a
diagnostic for explaining the notification's behaviour.

## Unit of counting

A single observation is one **(session, crossing) pair**. Each pair contributes at most one
outcome, whether the approach lasted one fix or two hundred.

Counting per prediction cycle instead is not permitted. It weights an observation by how
many fixes the approach happened to contain, so slow approaches and densely sampled
sessions dominate any average, and the denominator becomes a quantity the predictor
influences. This is the failure that point-adjusted scoring exhibits in time-series
detection benchmarks.

## Ground truth

The positives are the crossings recorded as passed in a session, one row per
(session, crossing).

The crossing instant is obtained by **interpolating along the session path**: the crossing
is projected onto the session's metric geometry and the instant interpolated between the
two fixes bracketing that position. Snapping to the nearest fix is not sufficient — at line
speed the nearest fix may be over a hundred metres from the crossing, which places several
seconds of error in the reference before any tolerance is applied.

## Outcome classification

For each pair, the predictor together with a trigger policy either raises an alert or does
not. Where an alert is raised, its **lead time** is the interval between the alert firing
and the crossing instant. Only the first alert per pair is considered.

| Outcome | Condition |
| --- | --- |
| Hit | alert raised, lead time within the window |
| Late | alert raised, lead time below the window |
| Early | alert raised, lead time above the window |
| Miss | crossing passed, no alert raised |
| False alarm | alert raised for a crossing not passed in that session |

The window is asymmetric around the target lead time, following the convention for
scheduled-service punctuality, where arriving early and arriving late are distinct failures
rather than symmetric error. An initial window of 30–120 seconds around a 60-second target
is assumed and revised from the observed distribution.

`Early`, `Late`, and `False alarm` are all counted against the predictor. A crossing never
passed in the session is the class that a definition conditioned on an actual crossing
instant cannot express, and it is the class that most affects whether the notification is
tolerable in use.

## Headline measures

Three quantities, each a ratio of counts:

- **Useful alert rate** — hits divided by all crossings passed. Detection with timeliness
  included, so it cannot be raised by alerting indiscriminately.
- **False alarm ratio** — all non-hit alerts divided by all alerts raised.
- **False alarms per hour** — false alarms divided by total session duration. The measure
  closest to what the notification costs in use.

The first two combine into a single tracking figure, hits divided by (hits + non-hit alerts
+ misses). This is the critical success index used in forecast verification, which
disregards true negatives — appropriate where the negative space is every crossing in the
region at every instant. It is arithmetically the Jaccard index, so the standard
classification metrics libraries compute it, and F1 alongside it, from the same per-pair
binary labels.

## Operating point

The trigger threshold is swept rather than fixed in advance. For each candidate threshold,
useful alert rate is plotted against false alarms per hour, one point per threshold. The
operating point is chosen from that plot, and the evidence for the choice recorded.

## Horizon diagnostic

Where the headline measures are poor, the explanation comes from the time estimate itself.
Predictions are bucketed by **true remaining time to the crossing** and the signed estimate
error reported per bucket, as median and interquartile range.

Signed rather than absolute, so a systematic lean early or late is visible. Median and
quartiles rather than mean and standard deviation, because an unscheduled stop produces
outliers that distort a mean. Reporting the error stratified by horizon is necessary: a
figure averaged across horizons conceals the near-approach region, which is the only region
the notification depends on, behind a long tail of early estimates that cost nothing.

## Required alongside any result

- **The sample size**, at both levels: sessions, and (session, crossing) pairs. At the
  volumes available early, the interval around a rate is wide enough to contain most of the
  plausible range, and a result reported without its n will be read as more settled than it
  is.
- **A baseline.** A rate without something to compare it against carries no information.
  The minimum baseline is constant velocity taken from the reported speed of each fix,
  which is the direct ablation of a predictor deriving velocity from successive distances.
- **The tuning applied** — trigger threshold, lead-time window, match radius — since each
  is a chosen constant that the result is conditional on.

## Not covered

Deliberately outside this framework, in each case cheaper to add later than to remove:

- Uncertainty on a prediction. Estimates are points, so there is no calibration or
  reliability analysis.
- Repeat alerts for the same pair. The first is scored and the rest disregarded.
- Ambiguity between parallel lines in the ground truth, which persists until a session is
  matched to track.
- Attribution of a miss between predictor error and genuinely unpredictable behaviour of
  the vehicle.

## Approaches considered and rejected

**Per-cycle precision and recall over a (crossing × time) space.** The framing is sound —
the imbalance makes precision and recall the right family, over accuracy or ROC — but three
definitional problems make the numbers unresponsive to predictor quality. Recall defined as
"was any prediction made" is satisfied by construction by a predictor emitting for every
candidate within its search radius, so it measures the relation between the search radius
and the ground-truth match radius. Precision conditioned on an actual crossing instant
cannot score a prediction for a crossing never reached. And per-cycle counting carries the
denominator bias described above.

**Prognostics metrics** (α-λ accuracy, prognostic horizon, convergence). The domain is
structurally similar — repeated estimates of time to an event, refined as the event
approaches — and a tolerance proportional to remaining time is an attractive idea. Rejected
on three grounds. Those metrics assume prediction difficulty falls as the event nears,
which does not hold where a vehicle may stop without warning close to the crossing; the
result is a tolerance band tightest exactly where error is least reducible, penalising a
predictor for behaviour no causal predictor could anticipate. Prognostic horizon requires
an estimate to enter a band and remain inside it, so one late perturbation discards an
otherwise good approach. And the event is assumed certain to occur, leaving no place for a
candidate crossing that is never reached — which here is a principal failure class. The
comprehension cost is also not repaid: the same questions are answered by counting.

**Forecast verification** contributes the counting discipline retained above — a
contingency table per lead time, true negatives disregarded — being built for a process
that is non-monotonic, multi-target, and where the event may not occur at all.

## References

- [49 CFR 273.5 — On-time performance and train delays](https://www.ecfr.gov/current/title-49/subtitle-B/chapter-II/part-273/section-273.5)
  and [Runtime vs. On Time Performance](https://trapezegroup.com/fixed-route-scheduling/public-transit-performance-metrics-runtime-vs-on-time-performance/)
  — punctuality windows, and why they are asymmetric
- [POD, FAR and CSI](https://glossary.ametsoc.org/wiki/POD,_FAR,_and_CSI) and
  [WMO verification guidance](https://old.wmo.int/aemp/sites/default/files/MET_capability_demonstration_appa_verification.pdf)
  — contingency-table verification stratified by lead time
- [A review of travel and arrival-time prediction methods on road networks](https://pmc.ncbi.nlm.nih.gov/articles/PMC8444094/)
  — error measures conventional in arrival-time prediction
- [Navigating the Metric Maze](https://arxiv.org/pdf/2303.01272) — how point-wise and
  point-adjusted counting mislead in time-series detection
- [A comprehensive review and evaluation framework for data-driven prognostics](https://www.sciencedirect.com/science/article/pii/S0888327025007162)
  — the prognostics metric set, and its stated assumptions
