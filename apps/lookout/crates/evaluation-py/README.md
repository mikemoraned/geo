# evaluation-py

The evaluation's steps from python, as the extension module `lookout_evaluation`. A notebook runs
the steps the evaluation runs, so what it inspects is the evaluation rather than a copy of it.
`help(lookout_evaluation.pick)` in a notebook is the reference for what a step takes and returns,
and likewise for `session_rows`, `replay`, `score`, `summarise`, and `run`.

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

## Using it from a notebook

[The python extensions](../../README.md#the-python-extensions) covers taking an extension into a
notebook, and when uv rebuilds one. This one's package is `lookout-evaluation`.
`just test-python` (from `apps/lookout`) runs its tests.
