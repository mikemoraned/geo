# rerun-py

Replaying a recorded session into [rerun](https://rerun.io), through the predictor itself rather
than a second one written in python: `lookout_predictor` is the extension module binding it, and
`runner/` is the python that drives it. The runner is python because the rerun SDK carries more of
the blueprint API there than in Rust. What the predictor does is
[`predictor`](../../predictor/README.md); where a replay sits in the pipeline is
[architecture.md](../../../docs/architecture.md).

Each function's own documentation is [its docstring](../../../README.md#the-python-extensions).
Nothing is serialised across the boundary: python holds the state machine, and a call into it runs
the predictor's own code, in the `f64` the store holds and a python float is. Instants are aware
datetimes in whatever timezone the caller has them.

```
just sessions                   # the sessions the store holds
just replay <session-id>        # draw one in a viewer already running, started with `rerun`
```

```python
from lookout_predictor import CrowFlies

from runner.replay import replay
from runner.store import Store

store = Store()
country = store.country_of(session_id)
predictor = CrowFlies(store.crossings(country))
for step in replay(predictor, store.samples(session_id, country)):
    print(step.sample.t, step.predictions)
```

## The runner

`store.py` reads a session's samples and the crossings to scan them against, `replay.py` feeds
those samples through a predictor the caller built, `log.py` draws that into a recording,
`blueprint.py` lays out the views a recording opens as, and `main.py` is the command that puts
them together.

Which predictor to run, and what to give it, is the caller's: `replay` takes anything answering
`observe_sample` and `predictions`, since comparing those answers is what a replay is for. It is
lazy, so a session is drawn as it is replayed rather than collected first, and the predictor is
left holding the last fix it was given.

Samples come back ordered by instant and then by sequence, since two share an instant where a
device reports faster than its clock resolves. Every field past a position is what the device
happened to report: what it left out stays unknown rather than being invented.

Nothing here spells out a path or decodes WKB: `lookout_medallion.query_silver` registers a
dataset by name, so which files hold it and what CRS its geometry is in stay the store's to know.
A dataset never derived raises rather than reading as one with no rows — a store without crossings
cannot be replayed against, and saying so is more use than an empty map.

## Drawing into a viewer

A replay draws into the recording it is handed, never the one rerun holds globally, so a test
reads back what was drawn by standing in for one. The recording holds two maps — where the session
went, and the crossings it expected to reach — beside the accuracy each fix reported, over a log
of the steps.

It draws into a running viewer rather than into a file. A viewer reads only recordings from its
own minor version and the one before it, so a `.rrd` kept longer than that is a file nothing will
open.

The command blocks until the stream has flushed, so the process outlives what it is still sending:
a viewer that receives half a session is worse than one that waits for all of it. A store whose
sessions or crossings have never been derived is an ordinary state to find it in, so the command
says so in a line rather than a stack.

The viewer is checked for before drawing. A stream with nowhere to send drops what it is given and
says nothing, so without that check a replay into no viewer is silent and looks like success.

## Running the tests

`just test-python` (from `apps/lookout`) runs them, or `uv run --reinstall-package
lookout-predictor pytest -q` from here. [The app's README](../../../README.md#testing) says why
the rebuild is forced.
