# session-crossings-py

The session crossings matching from python, as the extension module `lookout_session_crossings`.
It runs the matching's own steps, so a notebook draws what the derivation does rather than a copy
of it. [session_crossings](../session_crossings/README.md) states the rule the steps apply.

`help(lookout_session_crossings.steps)` in a notebook is the reference for what each table holds.

```python
import pyarrow as pa
import lookout_medallion
import lookout_session_crossings

samples = lookout_medallion.query_silver(
    "SELECT t, geometry_projected FROM session_sample WHERE session_id = $session",
    country="DE",
    params={"session": session_id},
)
crossings = lookout_medallion.query_silver(
    "SELECT crossing_id, geometry, geometry_projected FROM water_crossing_v2",
    country="DE",
)
steps = lookout_session_crossings.steps(session_id, samples, crossings, radius_metres=250.0)
near, passes = pa.table(steps["near"]), pa.table(steps["passes"])
```

Both tables cross as Arrow, through the PyCapsule interface, in either direction. The samples and
the crossings arrive shaped as the store holds them, so a query's answer passes straight in.

## Using it from a notebook

[The python extensions](../../README.md#the-python-extensions) covers taking an extension into a
notebook, and when uv rebuilds one. This one's package is `lookout-session-crossings`.
`just test-python` (from `apps/lookout`) runs its tests.
