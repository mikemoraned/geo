# Choosing the predictor

*Report of 2026-11-10, from the evaluation runs in gold. It records each run, what the runs can
tell apart, and the predictor and settings the apps run as a result.*

The apps run CrowFlies with a radius of 5 km and a horizon of 5 minutes, as `platform-core` builds
it. Its run is marked *(apps)* below.

## Runs

### 1000 sessions in DE, pick seed 1

| Setting | Value |
| --- | --- |
| Runs | `09f24e97-2e2c-56f3-8c71-7f9e8ff48eb6`, `dde3b0ed-5380-5bef-aa52-7a9878f7c4d5`, `0d1df184-cb23-524d-bac0-631aa685d823`, `2201a262-a0fc-5a69-a78a-b2576b9f6bc9`, `71643c94-e994-5e24-b441-8f894005b6a2`, `903347e0-fb5b-5fdf-a5e8-ae34113d329c`, `fc0f0c49-bfb6-57fb-871a-095dff0d2b57`, `22d7d4cf-21ae-5fbb-8474-186a25939144`, `0a27ba0f-d395-5439-8733-a63f9ea7df8b`, in the order of the table below |
| Evaluation version | `replay-v1` |
| Sessions | 1000 in DE, `realtime_percentage` ≥ 90, pick seed 1, 904.6 hours |
| Pass detector | offline |
| Scoring | horizon 5 min, last moment 30 s, ramp 1 min, last-moment score 0.25, floor 0.05, warm-up 1 min |
| Bootstrap | 1000 resamples, seed 1 |

| Predictor | Float | Precision | Recall | False alarms / h | Lead p10 / median / p90 | Passes | Notifications | False alarms | Repeats | Misses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| oracle, horizon 5 min | f32 | 1.000 [1.000, 1.000] | 1.000 [1.000, 1.000] | 0.000 [0.000, 0.000] | 300 / 300 / 300 s | 12,082 | 12,082 | 0 | 0 | 0 |
| CrowFlies, radius 1 km, horizon 5 min | f32 | 0.323 [0.313, 0.332] | 0.844 [0.816, 0.868] | 19.7 [18.4, 21.1] | 20 / 40 / 90 s | 12,082 | 31,593 | 17,847 | 1,664 | 0 |
| CrowFlies, radius 2 km, horizon 5 min | f32 | 0.270 [0.261, 0.278] | 0.899 [0.888, 0.911] | 27.9 [26.1, 29.6] | 40 / 80 / 210 s | 12,082 | 40,284 | 25,269 | 2,933 | 0 |
| CrowFlies, radius 3 km, horizon 5 min | f32 | 0.215 [0.206, 0.224] | 0.860 [0.845, 0.874] | 35.7 [33.4, 37.7] | 60 / 130 / 320 s | 12,082 | 48,281 | 32,279 | 3,920 | 0 |
| CrowFlies, radius 5 km, horizon 5 min *(apps)* | f32 | 0.137 [0.127, 0.146] | 0.711 [0.686, 0.734] | 49.5 [46.4, 52.4] | 110 / 240 / 490 s | 12,082 | 62,791 | 44,802 | 5,907 | 0 |
| CrowFlies, radius 7.5 km, horizon 5 min | f32 | 0.083 [0.076, 0.091] | 0.514 [0.483, 0.547] | 60.7 [56.9, 64.1] | 170 / 330 / 600 s | 12,082 | 74,455 | 54,882 | 7,491 | 0 |
| CrowFlies, radius 10 km, horizon 5 min | f32 | 0.069 [0.063, 0.074] | 0.444 [0.416, 0.472] | 64.4 [60.4, 68.2] | 220 / 360 / 620 s | 12,082 | 78,285 | 58,291 | 7,912 | 0 |
| CrowFlies, radius 15 km, horizon 5 min | f32 | 0.066 [0.061, 0.071] | 0.431 [0.405, 0.456] | 65.3 [61.1, 69.3] | 230 / 360 / 630 s | 12,082 | 79,085 | 59,053 | 7,950 | 0 |
| CrowFlies, radius 20 km, horizon 5 min | f32 | 0.066 [0.060, 0.071] | 0.431 [0.405, 0.456] | 65.4 [61.2, 69.4] | 230 / 360 / 630 s | 12,082 | 79,228 | 59,196 | 7,950 | 0 |

The oracle reads ground truth, which the apps cannot, so it can never be chosen. It shows the best
score the data and the rules allow.

### All 34,521 sessions in DE

| Setting | Value |
| --- | --- |
| Runs | `27013fff-1ae7-58c3-81ee-0098a4461cbd`, `35d02716-001e-5aab-b004-62fd02642d53`, `8d19431d-16ad-5baf-bdc6-aa5f58a68510`, in the order of the table below |
| Evaluation version | `replay-v1` |
| Sessions | all 34,521 in DE with `realtime_percentage` ≥ 90, 30,653.9 hours |
| Pass detector | offline |
| Scoring | horizon 5 min, last moment 30 s, ramp 1 min, last-moment score 0.25, floor 0.05, warm-up 1 min |
| Bootstrap | 1000 resamples, seed 1 |

| Predictor | Float | Precision | Recall | False alarms / h | Lead p10 / median / p90 | Passes | Notifications | False alarms | Repeats | Misses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CrowFlies, radius 1 km, horizon 5 min | f32 | 0.327 [0.326, 0.329] | 0.862 [0.858, 0.866] | 20.3 [20.0, 20.5] | 20 / 40 / 80 s | 415,707 | 1,094,457 | 621,220 | 57,530 | 0 |
| CrowFlies, radius 2 km, horizon 5 min | f32 | 0.269 [0.268, 0.271] | 0.903 [0.901, 0.905] | 28.6 [28.3, 28.9] | 40 / 90 / 210 s | 415,707 | 1,393,957 | 877,225 | 101,025 | 0 |
| CrowFlies, radius 3 km, horizon 5 min | f32 | 0.212 [0.210, 0.213] | 0.859 [0.857, 0.861] | 36.9 [36.5, 37.3] | 70 / 140 / 320 s | 415,707 | 1,684,952 | 1,131,825 | 137,420 | 0 |

Every session the filter keeps is in these runs, so the pick seed chooses nothing.

## Comparison

Eight runs of CrowFlies differ only in radius. Against the oracle, every one of them falls short on
every measure, and no interval overlaps the oracle's.

- **Recall:** 2 km scores highest, above every other radius. 1 km and 3 km cannot be told apart.
  Each radius from 3 km to 10 km scores below the one before it. 10 km, 15 km, and 20 km cannot be
  told apart.
- **False alarms per hour:** each radius up to 7.5 km raises false alarms above the one before
  it. 7.5 km, 10 km, 15 km, and 20 km overlap where they meet, so the runs cannot order them.
- **Precision:** each radius up to 10 km lowers precision below the one before it. 10 km, 15 km,
  and 20 km cannot be told apart.

Past 10 km the radius barely matters. CrowFlies alerts only a crossing it expects within the
5-minute horizon, and only a train above 120 km/h covers 10 km in 5 minutes, so the horizon
decides first.

Over all 34,521 sessions, the three radii keep the order the 1000 sessions gave. 2 km scores the
highest recall, and 1 km and 3 km still cannot be told apart on it. Each larger radius raises false
alarms and lowers precision. Every value lies inside the interval its run over 1000 sessions gave,
and each interval is about six times narrower.

## Trade-offs

No larger radius beats 1 km or 2 km on any measure. 2 km is better than 3 km on all three measures. 1 km
matches 3 km on recall and beats it on precision and false alarms.

Between 1 km and 2 km, the choice trades recall against false alarms. 2 km raises recall from 0.844
to 0.899, and adds 8 false alarms per hour, from 19.7 to 27.9. It also doubles the median lead
time, from 40 s to 80 s. At 1 km, a rider hears of half the passes 40 s or less ahead, which leaves
little time to act.

A larger radius loses recall as well as precision. It alerts a crossing sooner. From 7.5 km up, a
tenth of the notifications the passes credit come 10 minutes or more before their pass. A pass
credits its worst notification, so an alert too early lowers recall even when a later one comes on
time. The radius cannot buy lead time without that cost.

Every radius leaves about 20 or more false alarms per hour, one every three minutes at best. No
setting of CrowFlies makes an alert a rider can trust. The map of false alarms in the next task
shows whether they lie off the train's track or behind it, which a heading would rule out.

I recommend CrowFlies at 2 km and 5 minutes. It has the highest recall of any radius, and a lead
time a rider can act on. The runs over every session confirm it: 2 km raises recall from 0.862 to
0.903 over 1 km, at 28.6 false alarms per hour against 20.3.

## Choice

After the oracle run, the apps kept CrowFlies, with a radius of 5 km and a horizon of 5 minutes. No
run then measured a predictor the apps can run, so nothing argued for a change.

After the CrowFlies run, the apps keep CrowFlies, with a radius of 5 km and a horizon of 5
minutes. It is the only candidate with a run, and the radius sweep gives the settings to choose
between.

After the radius sweep, the apps change to CrowFlies with a radius of 2 km and a horizon of 5
minutes. It has the highest recall of any radius, at 0.899, and a median lead time of 80 s. It
costs 27.9 false alarms per hour, against 49.5 at 5 km.

After the runs over every session, the apps change to CrowFlies with a radius of 1 km and a
horizon of 5 minutes, in place of the 2 km chosen after the sweep. The user favours precision and
few false alarms over recall. 1 km scores the highest precision of any radius, at 0.327 against
0.269 at 2 km, and the fewest false alarms, at 20.3 per hour against 28.6. It gives up recall, from
0.903 to 0.862, and median lead time, from 90 s to 40 s.
