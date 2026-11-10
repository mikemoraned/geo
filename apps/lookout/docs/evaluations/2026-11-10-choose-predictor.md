# Choosing the predictor

*Report of 2026-11-10, from the evaluation runs in gold. It records each run, what the runs can
tell apart, and the predictor and settings the apps run as a result.*

The apps run CrowFlies with a radius of 5 km and a horizon of 5 minutes, as `platform-core` builds
it.

## Runs

No two runs are comparable yet, so each run stands alone.

### Reference: the oracle

The oracle reads ground truth, which the apps cannot, so it can never be chosen. It shows the best
score the data and the rules allow.

| Setting | Value |
| --- | --- |
| Run | `09f24e97-2e2c-56f3-8c71-7f9e8ff48eb6` |
| Evaluation version | `replay-v1` |
| Sessions | 1000 in DE, `realtime_percentage` ≥ 90, pick seed 1 |
| Pass detector | offline |
| Scoring | horizon 5 min, last moment 30 s, ramp 1 min, last-moment score 0.25, floor 0.05, warm-up 1 min |
| Bootstrap | 1000 resamples, seed 1 |

| Predictor | Float | Precision | Recall | False alarms / h | Lead p10 / median / p90 | Passes | Notifications | False alarms | Repeats | Misses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| oracle, horizon 5 min | f32 | 1.000 [1.000, 1.000] | 1.000 [1.000, 1.000] | 0.000 [0.000, 0.000] | 300 / 300 / 300 s | 12,082 | 12,082 | 0 | 0 | 0 |

The 1000 sessions cover 904.6 hours.

## Comparison

No group holds two predictors, so the runs compare nothing.

## Trade-offs

No run measures a predictor the apps can run, so the runs leave the choice open. The CrowFlies run
over the same sessions and seed is the first that can inform it.

## Choice

The apps keep CrowFlies, with a radius of 5 km and a horizon of 5 minutes. No run measures a
predictor the apps can run, so nothing argues for a change. The choice changes nothing the apps
run.

