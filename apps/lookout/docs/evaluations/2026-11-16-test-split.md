# LightCone against CrowFlies on the test split

This report compares the settings chosen for each predictor on the tune split, scored once on the
test split. It records the choice of the apps' predictor, made from these runs alone.

## Runs

### Every session, test split

Shared settings: evaluation `replay-v1`, country DE, realtime at least 90%, 34,521 sessions, pick
seed 1. The runs score the test split at A5 resolution 8, salt `tune-test-1`, and 70 tune
buckets. Scoring: horizon 5 min, last moment 30 s, ramp 1 min, last-moment score 0.25, floor 0.05,
warm-up 1 min. Both runs use `f32`, a radius of 1 km, a predictor horizon of 5 min, and `LastMove`
at 20 m.

Run ids: `7ff72ea5-efff-5c07-816b-eab468d12888`, `92e6ba69-e0ae-5281-ae79-4c2a1f6e7b46`.

★ marks the predictor the apps run.

| Run | Predictor | Half-width | Precision | Recall | False alarms / h | F0.5 | Lead p10 / median / p90 | Passes | Notifications | False alarms | Repeats | Misses |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `7ff72ea5` ★ | CrowFlies | — | 0.366 [0.363, 0.368] | 0.906 [0.901, 0.910] | 18.77 [18.46, 19.07] | 0.415 [0.413, 0.418] | 20 / 40 / 100 s | 115,818 | 286,936 | 165,084 | 6,034 | 0 |
| `92e6ba69` | LightCone | 45° | 0.677 [0.672, 0.682] | 0.893 [0.889, 0.897] | 3.32 [3.21, 3.43] | 0.711 [0.706, 0.716] | 20 / 40 / 90 s | 115,818 | 152,847 | 29,215 | 7,885 | 71 |

## Skew

Skew is how far the passes the apps can see depart from ground truth. It describes the detectors
and the data, not the predictor, so it plays no part in the comparison or the choice.

| Runs | Online pass radius | Seen | Unmatched | Offset p10 / median / p90 | Passes | Passes seen | Visits | Visits unmatched |
|---|---|---|---|---|---|---|---|---|
| Both runs | 100 m | 0.756 [0.751, 0.761] | 0.000 [0.000, 0.000] | 0 / 0 / 0 s | 124,136 | 93,864 | 93,864 | 0 |

## Comparison

LightCone and CrowFlies differ on every measure, with disjoint intervals. LightCone scores higher
on precision, 0.677 against 0.366, and on F0.5, 0.711 against 0.415. It raises 3.32 false alarms
an hour against 18.77. CrowFlies scores higher on recall, 0.906 against 0.893, and misses no pass,
where LightCone misses 71.

The intervals are unpaired, and treat sessions as independent. Neither changes the comparison,
since every gap far exceeds its interval.

## Trade-offs

LightCone ranks first by F0.5. It gives up 0.013 recall and 71 of 115,818 passes, and raises a
sixth of CrowFlies' false alarms per hour. Both predictors alert with the same lead time.

Each predictor scores within 0.01 of its tune score on F0.5, so the settings chosen on the tune
split hold on the test split.

LightCone at 1 km and 45°, with `LastMove` at 20 m, is the recommendation for the apps.

## Choice

After the runs on the test split, the apps change to LightCone, with a radius of 1 km, a
half-width of 45°, `LastMove` at 20 m, and a horizon of 5 minutes. The user took the
recommendation. LightCone scores higher than CrowFlies on F0.5 and raises a sixth of its false
alarms per hour, for a small loss of recall.
