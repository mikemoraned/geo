# lookout_predictor

The crow-flies predictor as a python extension module, so the rerun runner replays a recorded
session through the predictor itself rather than through a second one written in python. See
`src/lib.rs` for the API, whose doc comments are the module's `__doc__`, and
[`docs/architecture.md`](../../../docs/architecture.md) for where it sits.

## The runner

`runner/` is the python beside the extension: `store.py` reads a session's samples and the
crossings to scan them against, through `lookout_medallion.query_silver`; `replay.py` feeds
those samples through a predictor the caller built, a step per sample; `log.py` draws that
into a recording; `blueprint.py` lays out the views a recording opens as; and `main.py` is the
command that puts them together.

A replay draws through the recording it is handed, never through the one rerun holds globally,
so a test reads back what was drawn by standing in for one.

```
just sessions                   # the sessions the store holds
just replay <session-id>        # replay one into a .rrd beside the store
```

```python
from lookout_predictor import CrowFlies

from runner.replay import replay
from runner.store import Store

store = Store()
predictor = CrowFlies(store.crossings(country="DE"))
for step in replay(predictor, store.samples(session_id)):
    print(step.sample.t, step.predictions)
```

## Running the tests

`just test-python` (from `apps/lookout`) runs them, or from this directory:

```
uv run --reinstall-package lookout-predictor pytest -q
```

Nothing needs installing first, since uv builds the extension with maturin. The rebuild is
forced because uv caches the built wheel against this crate's own sources, and would otherwise
miss a change to the rust crates it wraps.
