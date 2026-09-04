# lookout_predictor

The crow-flies predictor as a python extension module, so the rerun runner replays a recorded
session through the predictor itself rather than through a second one written in python — see
[`docs/architecture.md`](../../../docs/architecture.md) for where it sits, and `src/lib.rs` for
the API, whose doc comments are the module's `__doc__`.

```python
from lookout_predictor import CrowFlies

predictor = CrowFlies([(crossing_id, lat, lon), ...], radius_metres=5000.0)
predictor.observe_sample(t, lat, lon, speed_mps=speed)
for prediction in predictor.predictions():
    print(prediction.crossing, prediction.metres, prediction.at)
```

## What it expects

Samples arrive in `t` order, each instant tz-aware. One behind the clock raises a `ValueError`
and changes nothing; a naive datetime raises a `TypeError`. A coordinate off the globe raises a
`ValueError` too, whether it arrives as a crossing or as a fix.

Everything past the position is optional, and a field left out stays unknown rather than
becoming zero. Without a reported speed, the step from the previous fix says how fast we are
going. Without a previous fix, a prediction carries a distance and no time.

Distances are metres and speeds metres per second, measured in `f64` — what the store holds and
what a python float is.

## Running the tests

`just test-python` (from `apps/lookout`) runs them, or from this directory:

```
uv run --reinstall-package lookout-predictor pytest -q
```

Nothing needs installing first, since uv builds the extension with maturin. The rebuild is
forced because uv caches the built wheel against this crate's own sources, and would otherwise
miss a change to the rust crates it wraps.
