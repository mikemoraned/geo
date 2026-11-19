# evaluation-py

The evaluation's steps from python, as the extension module `lookout_evaluation`. A notebook runs
the steps the evaluation runs, so what it inspects is the evaluation rather than a copy of it.
`help(lookout_evaluation.pick)` in a notebook is the reference for what a step takes and returns,
and likewise for every other function and class.

```python
import pyarrow as pa
import lookout_evaluation as evaluation

scoring = evaluation.Scoring(
    horizon="5m", last_moment="30s", ramp="1m", last_moment_score=0.25, floor=0.05, warm_up="1m"
)
bootstrap = evaluation.Bootstrap(resamples=1000, seed=1)
oracle = evaluation.Oracle(horizon="5m")

picked = pa.table(evaluation.pick(country="DE", realtime_at_least=90.0, sessions=1000, seed=1))
sessions = pa.table(evaluation.session_rows(country="DE", picked=picked))
transitions = pa.table(evaluation.replay(country="DE", sessions=sessions, predictor=oracle))
outcomes = pa.table(
    evaluation.score(country="DE", sessions=sessions, transitions=transitions, scoring=scoring)
)
summary = pa.table(evaluation.summarise(sessions=sessions, outcomes=outcomes, bootstrap=bootstrap))
```

Each step takes the tables of the steps before it, so a notebook can inspect or change a table
between steps. `session_rows` turns the picked sessions into the sessions the later steps take,
with each session's duration and scored duration. `run` takes the arguments of every step, and
returns the four tables in a dict.
Given an `OnlineDetector`, `run` also returns a fifth table, `skew`: how far the passes the online
detector reports depart from ground truth. Skew describes the detectors, not the predictor.

A `ScoredSplit` scores one split of the crossings, `"tune"` or `"test"`, of the cells a `Splitter`
assigns. `session_rows`, `replay`, `score`, and `run` take it as `split`. A run of the steps one by
one passes each step the same split:

```python
splitter = evaluation.Splitter(resolution=8, salt="tune-test-1", tune_buckets=70)
test = evaluation.ScoredSplit(splitter=splitter, split="test")
sessions = pa.table(evaluation.session_rows(country="DE", picked=picked, split=test))
```

Given a split, `run` also returns a table under the key `split`: the counts its checks of the split
against the store compared. A split that fails a check raises a `ValueError`, as does a store
that holds no Overture extract of the country.

A step's table crosses as Arrow, through the PyCapsule interface. A table of transitions, outcomes,
a summary, a skew, or a split has the columns of the gold dataset of that name,
`evaluation_session_v3`, `evaluation_transition_v1`, `evaluation_outcome_v2`,
`evaluation_summary_v3`, `evaluation_skew_v2`, or `evaluation_split_v1`. A table handed back to
a step is read through those columns. A table that lacks one, or that names a session the step was
not given, raises a `ValueError`.

Gold stores a run's transitions and outcomes in fewer bytes than those tables take:
`evaluation_episode_v1` holds one row per episode, a stay of a crossing within the predictor's
reach, and `evaluation_outcome_v3` holds each outcome by session index, with times as offsets from
the session's start. `transitions_of` and `outcomes_of` rebuild the tables from a run's rows of
those datasets and of `evaluation_session_v3`:

```python
transitions = evaluation.transitions_of(sessions=sessions, episodes=episodes)
outcomes = evaluation.outcomes_of(sessions=sessions, outcomes=compact)
```

## Predictors

A step that replays takes a predictor: an `Oracle`, a `CrowFlies`, or a `LightCone`. Each takes
the settings the `evaluate` binary does, with spans of time as text such as `"5m"`:

```python
oracle = evaluation.Oracle(horizon="5m")
crow_flies = evaluation.CrowFlies(radius_metres=1000, horizon="5m", still_within_metres=20)
light_cone = evaluation.LightCone(
    radius_metres=1000, half_width_degrees=45, horizon="5m", still_within_metres=20
)
online = evaluation.OnlineDetector(pass_radius_metres=250)
```

## Writing a run to gold

`evaluate` runs as `run` does, and writes each step's table to gold. `record` records a run's
settings and sessions from a picked table, and returns its id, its date, and its first two tables,
which `write_gold` from [medallion-py](../medallion-py/README.md#gold-datasets-of-rows) then writes.
Both name the code the extension was built from, so each raises a `ValueError` if the extension was
built from uncommitted code. `evaluate` also raises one if gold already holds the run.

## Measuring how far a train turns

`cone_stages` runs the stages that measure how far a train turns from its heading within a horizon,
and returns each stage's table, as the `evaluation_cone_angles` binary computes them. `velocities`
returns the speed and heading LightCone estimates at each sample. Both take a table of samples as
silver `train_session_sample_v3` holds them.

## Using it from a notebook

[The python extensions](../../README.md#the-python-extensions) covers taking an extension into a
notebook, and when uv rebuilds one. This one's package is `lookout-evaluation`.
`just test-python` (from `apps/lookout`) runs its tests.
