# evaluation-py

The evaluation's steps from python, as the extension module `lookout_evaluation`. A notebook runs
the steps the evaluation runs, so what it inspects is the evaluation rather than a copy of it.
`help(lookout_evaluation.pick)` in a notebook is the reference for what a step takes and returns.

```python
import pyarrow as pa
import lookout_evaluation

sessions = pa.table(
    lookout_evaluation.pick(country="DE", realtime_at_least=90.0, sessions=1000, seed=1)
)
```

A step's table crosses as Arrow, through the PyCapsule interface.

## Using it from a notebook

[The python extensions](../../README.md#the-python-extensions) covers taking an extension into a
notebook, and when uv rebuilds one. This one's package is `lookout-evaluation`.
`just test-python` (from `apps/lookout`) runs its tests.
