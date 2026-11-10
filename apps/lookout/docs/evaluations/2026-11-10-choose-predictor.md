# Choosing the predictor

*Report of 2026-11-10, from the evaluation runs in gold. It records each run, what the runs can
tell apart, and the predictor and settings the apps run as a result.*

The apps run CrowFlies with a radius of 5 km and a horizon of 5 minutes, as `platform-core` builds
it. Its run is marked *(apps)* below.

## Runs

### 1000 sessions in DE, pick seed 1

| Setting | Value |
| --- | --- |
| Runs | `09f24e97-2e2c-56f3-8c71-7f9e8ff48eb6`, `71643c94-e994-5e24-b441-8f894005b6a2` |
| Evaluation version | `replay-v1` |
| Sessions | 1000 in DE, `realtime_percentage` ≥ 90, pick seed 1, 904.6 hours |
| Pass detector | offline |
| Scoring | horizon 5 min, last moment 30 s, ramp 1 min, last-moment score 0.25, floor 0.05, warm-up 1 min |
| Bootstrap | 1000 resamples, seed 1 |

| Predictor | Float | Precision | Recall | False alarms / h | Lead p10 / median / p90 | Passes | Notifications | False alarms | Repeats | Misses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| oracle, horizon 5 min | f32 | 1.000 [1.000, 1.000] | 1.000 [1.000, 1.000] | 0.000 [0.000, 0.000] | 300 / 300 / 300 s | 12,082 | 12,082 | 0 | 0 | 0 |
| CrowFlies, radius 5 km, horizon 5 min *(apps)* | f32 | 0.137 [0.127, 0.146] | 0.711 [0.686, 0.734] | 49.5 [46.4, 52.4] | 110 / 240 / 490 s | 12,082 | 62,791 | 44,802 | 5,907 | 0 |

The oracle reads ground truth, which the apps cannot, so it can never be chosen. It shows the best
score the data and the rules allow.

## Comparison

The group holds one predictor the apps can run, so no two candidates compare.

Against the oracle, CrowFlies falls short on every measure, and no interval overlaps the oracle's.
CrowFlies notifies five times for each pass, and 71% of its notifications are false alarms. It
misses no pass outright. Its lead times spread from 110 s to 490 s around the 5-minute horizon,
so many passes earn only part of their credit.

## Trade-offs

At 49.5 false alarms per hour, CrowFlies alerts a rider almost once a minute about a crossing the
train never passes. Its recall of 0.71 comes at that cost. A radius sweep trades the two against
each other. A smaller radius cuts false alarms, and a larger one reaches further ahead at speed.

I recommend keeping CrowFlies at 5 km and 5 minutes until the sweep gives settings to choose
between. No other candidate has a run yet.

## Choice

After the oracle run, the apps kept CrowFlies, with a radius of 5 km and a horizon of 5 minutes. No
run then measured a predictor the apps can run, so nothing argued for a change.

After the CrowFlies run, the apps keep CrowFlies, with a radius of 5 km and a horizon of 5
minutes. It is the only candidate with a run, and the radius sweep gives the settings to choose
between.
