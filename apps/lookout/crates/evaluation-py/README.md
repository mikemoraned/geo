# evaluation-py

The evaluation's steps from python, as the extension module `lookout_evaluation`. A notebook runs
the steps the evaluation runs, so what it inspects is the evaluation rather than a copy of it.
`help(lookout_evaluation.pick)` in a notebook is the reference for what a step takes and returns,
and likewise for `replay`, `score`, `summarise`, and `run`.

```python
import pyarrow as pa
import lookout_evaluation as evaluation

scoring = evaluation.Scoring(
    horizon="5m", last_moment="30s", ramp="1m", last_moment_score=0.25, floor=0.05, warm_up="1m"
)
bootstrap = evaluation.Bootstrap(resamples=1000, seed=1)
oracle = evaluation.Oracle(horizon="5m")

sessions = pa.table(evaluation.pick(country="DE", realtime_at_least=90.0, sessions=1000, seed=1))
transitions = pa.table(evaluation.replay(country="DE", sessions=sessions, predictor=oracle))
outcomes = pa.table(
    evaluation.score(country="DE", sessions=sessions, transitions=transitions, scoring=scoring)
)
summary = pa.table(evaluation.summarise(sessions=sessions, outcomes=outcomes, bootstrap=bootstrap))
```

Each step takes the tables of the steps before it, so a notebook can inspect or change a table
between steps. `run` takes the arguments of every step, and returns the four tables in a dict.

A step's table crosses as Arrow, through the PyCapsule interface. A table of transitions, outcomes,
or a summary has the columns of the gold dataset of that name, `evaluation_transition_v1`,
`evaluation_outcome_v1`, or `evaluation_summary_v1`. A table handed back to a step is read through
those columns. A table that lacks one, or that names a session the step was not given, raises a
`ValueError`.

## Using it from a notebook

[The python extensions](../../README.md#the-python-extensions) covers taking an extension into a
notebook, and when uv rebuilds one. This one's package is `lookout-evaluation`.
`just test-python` (from `apps/lookout`) runs its tests.
