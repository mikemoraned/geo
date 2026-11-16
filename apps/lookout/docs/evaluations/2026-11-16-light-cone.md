# LightCone against CrowFlies on the tune split

This report compares LightCone at half-widths from 15° to 90° with CrowFlies at radii of 1 km to
3 km. Every run scores the tune split. It records the choice of each predictor's settings for the
apps.

## Runs

### Every session, tune split

Shared settings: evaluation `replay-v1`, country DE, realtime at least 90%, 34,521 sessions, pick
seed 1. The runs score the tune split at A5 resolution 8, salt `tune-test-1`, and 70 tune
buckets. Scoring: horizon 5 min, last moment 30 s, ramp 1 min, last-moment score 0.25, floor 0.05,
warm-up 1 min. Every run uses `f32`.

Run ids: `788ea2a4-6901-5881-9e87-bdf8c13a280b`, `8a1c128f-4f78-5aed-81fb-7f0fca3ac7db`,
`731f0149-ace8-5e9f-8d3b-c2b033d63bd7`, `c7ce1665-bc68-5d0c-afb3-90368a097231`,
`a039de09-213d-5fa2-9338-4160f879a544`, `a029d5b6-5510-5493-a11a-b5977e60a82c`,
`dad7dd8e-c312-5995-a5ab-14b7f610c0e0`, `e83b8549-0cc6-502a-bf60-97602e610b70`,
`545e271e-3980-5d24-8457-0a957c68716f`, `39253c48-5b74-5c9a-9aad-f3f5ca78461a`,
`00e77d0f-36dc-5902-88f8-937874acb9e6`, `882111f4-c49d-5a18-b1d5-d3c848cccd90`,
`cd2de653-b05f-5978-90a6-866dfc8e0ded`, `b36e97b6-d1b7-5f84-965c-f558045640ce`,
`21b57e06-49e4-5eb4-9be7-ebe4c758e736`.

The radius is CrowFlies' alerting radius, or the radius LightCone filters by bearing. "Still
within" is `LastMove`'s radius. A run without it takes the speed the receiver reports. The first
five LightCone runs predate its rebuild on CrowFlies, and the first three CrowFlies runs predate
`LastMove`. Both sets measure code the apps no longer hold. ★ marks the predictor the apps run.

| Run | Predictor | Radius | Half-width | Still within | Precision | Recall | False alarms / h | F0.5 | Lead p10 / median / p90 | Passes | Notifications | False alarms | Repeats | Misses |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `788ea2a4` | LightCone, before rebuild | 20 km | 30° | 20 m | 0.104 [0.102, 0.106] | 0.472 [0.468, 0.477] | 28.71 [28.22, 29.22] | 0.123 [0.121, 0.125] | 90 / 330 / 600 s | 299,889 | 1,360,749 | 627,729 | 433,436 | 305 |
| `8a1c128f` | LightCone, before rebuild | 20 km | 45° | 20 m | 0.100 [0.098, 0.101] | 0.449 [0.444, 0.453] | 32.14 [31.61, 32.69] | 0.118 [0.116, 0.120] | 180 / 350 / 640 s | 299,889 | 1,351,568 | 702,618 | 349,331 | 270 |
| `731f0149` | LightCone, before rebuild | 20 km | 60° | 20 m | 0.098 [0.096, 0.099] | 0.441 [0.436, 0.445] | 35.72 [35.16, 36.32] | 0.116 [0.114, 0.117] | 190 / 350 / 660 s | 299,889 | 1,353,501 | 780,889 | 272,943 | 220 |
| `c7ce1665` | LightCone, before rebuild | 20 km | 75° | 20 m | 0.098 [0.096, 0.099] | 0.442 [0.438, 0.447] | 38.07 [37.50, 38.66] | 0.116 [0.114, 0.118] | 200 / 360 / 660 s | 299,889 | 1,358,026 | 832,345 | 226,011 | 219 |
| `a039de09` | LightCone, before rebuild | 20 km | 90° | 20 m | 0.095 [0.094, 0.097] | 0.442 [0.437, 0.447] | 40.91 [40.29, 41.53] | 0.113 [0.111, 0.115] | 200 / 360 / 660 s | 299,889 | 1,393,282 | 894,288 | 199,311 | 206 |
| `a029d5b6` | CrowFlies | 1 km | — | — | 0.327 [0.325, 0.328] | 0.859 [0.855, 0.864] | 20.39 [20.11, 20.65] | 0.373 [0.371, 0.375] | 20 / 40 / 90 s | 299,889 | 789,166 | 445,660 | 43,617 | 0 |
| `dad7dd8e` | CrowFlies | 2 km | — | — | 0.263 [0.262, 0.265] | 0.897 [0.895, 0.900] | 29.64 [29.26, 30.00] | 0.307 [0.305, 0.309] | 40 / 90 / 220 s | 299,889 | 1,021,578 | 647,971 | 73,718 | 0 |
| `e83b8549` | CrowFlies | 3 km | — | — | 0.206 [0.204, 0.207] | 0.852 [0.849, 0.855] | 38.55 [38.04, 39.03] | 0.242 [0.241, 0.244] | 70 / 140 / 330 s | 299,889 | 1,242,675 | 842,844 | 99,942 | 0 |
| `545e271e` ★ | CrowFlies | 1 km | — | 20 m | 0.361 [0.359, 0.363] | 0.897 [0.893, 0.901] | 19.33 [19.06, 19.58] | 0.410 [0.408, 0.412] | 20 / 40 / 110 s | 299,889 | 745,515 | 422,502 | 23,124 | 0 |
| `39253c48` | LightCone | 1 km | 15° | 20 m | 0.599 [0.596, 0.604] | 0.701 [0.698, 0.705] | 1.10 [1.07, 1.14] | 0.617 [0.614, 0.621] | 10 / 30 / 90 s | 299,889 | 350,828 | 24,120 | 42,034 | 15,215 |
| `00e77d0f` | LightCone | 1 km | 30° | 20 m | 0.642 [0.638, 0.646] | 0.820 [0.816, 0.824] | 1.99 [1.94, 2.03] | 0.671 [0.667, 0.675] | 20 / 40 / 100 s | 299,889 | 382,864 | 43,409 | 40,665 | 1,099 |
| `882111f4` | LightCone | 1 km | 45° | 20 m | 0.668 [0.664, 0.671] | 0.874 [0.871, 0.879] | 2.68 [2.62, 2.73] | 0.701 [0.697, 0.705] | 20 / 40 / 100 s | 299,889 | 392,632 | 58,564 | 34,537 | 358 |
| `cd2de653` | LightCone | 1 km | 60° | 20 m | 0.666 [0.662, 0.670] | 0.891 [0.887, 0.895] | 3.35 [3.28, 3.40] | 0.701 [0.697, 0.705] | 20 / 40 / 100 s | 299,889 | 401,293 | 73,134 | 28,495 | 225 |
| `b36e97b6` | LightCone | 1 km | 75° | 20 m | 0.663 [0.659, 0.667] | 0.898 [0.894, 0.902] | 3.80 [3.73, 3.86] | 0.700 [0.696, 0.704] | 20 / 40 / 100 s | 299,889 | 406,074 | 82,975 | 23,429 | 219 |
| `21b57e06` | LightCone | 1 km | 90° | 20 m | 0.658 [0.654, 0.662] | 0.898 [0.894, 0.902] | 4.00 [3.93, 4.07] | 0.695 [0.691, 0.699] | 20 / 40 / 100 s | 299,889 | 409,330 | 87,524 | 22,123 | 206 |

### Runs outside every group

`d99b29c2-b8b8-57d0-b5b0-942c0aef622c` runs LightCone before its rebuild, at a radius of 20 km, a
half-width of 45°, and `LastMove` at 20 m. It covers 1,000 sessions under pick seed 1 and shares
every other setting with the group above. It scores a precision of 0.104 [0.094, 0.113], a recall
of 0.456 [0.428, 0.482], 30.01 [27.07, 33.06] false alarms per hour, and an F0.5 of 0.123
[0.111, 0.133]. Its lead time runs 190 / 350 / 610 s. It counts 8,457 passes, 37,179
notifications, 19,066 false alarms, 9,672 repeats, and 16 misses.

## Skew

Skew is how far the passes the apps can see depart from ground truth. It describes the detectors
and the data, not the predictor, so it plays no part in the comparison or the choice.

| Runs | Online pass radius | Seen | Unmatched | Offset p10 / median / p90 | Passes | Passes seen | Visits | Visits unmatched |
|---|---|---|---|---|---|---|---|---|
| The 15 runs over every session | 100 m | 0.770 [0.766, 0.774] | 0.001 [0.001, 0.001] | 0 / 0 / 0 s | 314,781 | 242,297 | 242,480 | 183 |
| `d99b29c2` | 100 m | 0.752 [0.728, 0.775] | 0.002 [0.000, 0.005] | 0 / 0 / 0 s | 8,831 | 6,639 | 6,654 | 15 |

## Comparison

This section compares the runs of code the apps hold: CrowFlies with `LastMove`, and the rebuilt
LightCone at each half-width. The runs before the rebuild and before `LastMove` score below these
on F0.5, with intervals far apart.

- **LightCone at any half-width against CrowFlies at 1 km.** Every interval is disjoint.
  LightCone scores higher on precision and F0.5, and raises a fifth or fewer of the false alarms
  per hour. Its recall is lower at 15° to 60°, and matches at 75° and 90°. It misses passes
  CrowFlies never misses: 206 at 90°, rising to 15,215 at 15°.
- **LightCone, 45° against 60°.** Precision and F0.5 overlap, so the runs cannot tell them apart
  there. 45° raises fewer false alarms per hour, 2.68 against 3.35. 60° has higher recall, 0.891
  against 0.874.
- **LightCone, 60° to 90°.** Recall overlaps between 75° and 90°, and F0.5 between 60° and 75°.
  Each step wider adds false alarms per hour, with disjoint intervals.
- **LightCone, 15° and 30°.** Each falls below 45° on recall and F0.5, with disjoint intervals.

These intervals are unpaired, and treat sessions as independent. Sessions on the same route pass
the same crossings, so the intervals are narrower than they ought to be.

## Trade-offs

By F0.5, LightCone at 45° and 60° rank first and level, at 0.701. 75° follows at 0.700, then 90°,
30°, and 15°. CrowFlies at 1 km scores 0.410.

The choice among 45°, 60°, and 75° trades recall against false alarms per hour. Going from 45° to
60° gains 0.017 recall and 133 fewer misses, at 0.67 more false alarms an hour. Going on to 75°
gains 0.007 recall at a further 0.45 false alarms an hour.

LightCone at 45° is the recommendation. It ties for the best F0.5, and raises the fewest false
alarms per hour of the settings that tie. CrowFlies' setting stays at 1 km with `LastMove` at 20
m. It is the best CrowFlies by every measure, and the baseline the test runs compare against.

## Choice

After the runs on the tune split, LightCone's settings are a radius of 1 km, a half-width of 45°,
and `LastMove` at 20 m, with a horizon of 5 minutes. CrowFlies' settings stay at a radius of 1 km
and `LastMove` at 20 m. The user took the recommendation: 45° ties for the best F0.5 and raises
the fewest false alarms per hour of the settings that tie. The apps keep CrowFlies until the runs
on the test split choose between the two.
