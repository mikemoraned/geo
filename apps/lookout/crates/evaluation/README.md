# evaluation

Measures a predictor against recorded train sessions: it picks sessions, replays them through the
predictor, scores the notifications against the sessions' passes, and sums the scores into
measures. [evaluation.md](../../docs/evaluation.md) sets out what a run measures and why. This
README covers the crate and its binaries.

The steps are plain functions over rows, so a binary, a notebook through
[evaluation-py](../evaluation-py/README.md), and a test all run the same code. A run keeps each
step's rows in memory, and writes them to gold only at the end of the step.

## The `evaluate` binary

`evaluate` runs an evaluation into gold, whole or a step at a time. The binary needs the `cli`
feature, which brings in the argument parser and the async runtime, so a crate that takes the
library alone builds without them. `just evaluate` (from `apps/lookout`) builds and runs it:

```sh
just evaluate run --country DE --realtime-at-least 90 --sessions all --seed 1 \
  --predictor light_cone --predictor-horizon 5m --radius-metres 1000 --half-width-degrees 45 \
  --still-within-metres 20 --online-pass-radius-metres 250 --float-type f32 \
  --horizon 5m --last-moment 30s --ramp 1m --last-moment-score 0.25 --floor 0.05 --warm-up 1m \
  --resamples 1000 --bootstrap-seed 1 \
  --split tune --split-resolution 8 --split-salt tune-test-1 --tune-buckets 70
```

`just evaluate run --help` describes each setting. The subcommands are the steps:

| Subcommand | What it does |
| --- | --- |
| `run` | Picks sessions, then replays, scores, and summarises them, as one run |
| `pick` | Picks sessions, and records the run's settings and inputs |
| `replay` | Replays a picked run's sessions through its predictor |
| `score` | Scores a replayed run's notifications against its sessions' passes |
| `summarise` | Summarises a scored run into its measures and their intervals |

`pick` prints the run's id, and each later step takes it. A run of the steps one by one writes what
`run` writes.

## What a run refuses

- **Uncommitted code.** A run names the code it ran by git's hashes of that code, so a build from a
  tree with uncommitted changes cannot name its code, and is refused.
- **A run already in gold.** A run's id is a hash of its settings, its code, and a fingerprint of
  each input dataset. The same settings over the same code and data give the same id, and gold
  holds a run once.
- **A step from other code, or over changed silver.** Each step after `pick` checks that the build
  and the silver datasets match those the run recorded at its pick.
- **A broken split.** A run with a split checks it against the store before it replays and after
  it scores, as [checks of a split](../../docs/evaluation.md#checks-of-a-split) sets out.

## What a run writes

Each run writes one partition, `run_date=<date>/run_id=<id>`, to each of these gold datasets:

| Dataset | Rows |
| --- | --- |
| `evaluation_run_v6` | the run's settings, code, and input fingerprints |
| `evaluation_session_v3` | each session replayed, numbered in route order |
| `evaluation_episode_v1` | each stay of a crossing within reach, holding its transitions |
| `evaluation_outcome_v3` | each notification's or miss's outcome |
| `evaluation_summary_v3` | the measures and their intervals |
| `evaluation_skew_v2` | the skew between the pass detectors, given an online detector |
| `evaluation_split_v1` | the counts the split's checks compared, given a split |

Episodes and compact outcomes store a run in a quarter of the bytes that one row per transition
takes, as [the gold layer](../../docs/medallion.md#gold) sets out. `transition_rows_of` and
`outcome_rows_of` rebuild the transitions and outcomes the steps work on.

## The `evaluation_cone_angles` binary

`evaluation_cone_angles` measures how far a train turns from its heading within a horizon, over the
windows of picked sessions that lie wholly in one split. It reports each stage's counts and the
angles' percentiles, which bound the sweep of LightCone's half-width. It also needs the `cli`
feature. `--help` describes each setting.

## Testing

`just test-no-docker` (from `apps/lookout`) runs the crate's tests, which run each step over a
store the fixtures write. The `fixtures` feature lends those fixtures to another crate's tests.
