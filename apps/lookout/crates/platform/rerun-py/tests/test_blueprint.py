"""The layout a recording opens as.

A blueprint is a set of views over entity paths, and the way it breaks is silent: rename a
path and the view it fed goes empty, with nothing to say so. So what these check is that the
paths a replay draws and the paths the views cover are the same set.
"""

import rerun.blueprint as rrb
from lookout_predictor import CrowFlies

from runner.blueprint import blueprint
from runner.log import (
    ACTUAL_ETA,
    CROSSINGS,
    DISTANCE,
    ERROR,
    PASSING,
    PREDICTED_ETA,
    SESSION,
    drawings,
)
from runner.replay import replay
from runner.store import Store

from conftest import COUNTRY, SESSION as SESSION_ID

# What the views cover, as prefixes. Restated here rather than imported from the blueprint,
# since a test that reads its expectation off the thing it is testing checks nothing.
VIEWED = (SESSION, CROSSINGS, PREDICTED_ETA, ACTUAL_ETA, ERROR, DISTANCE, PASSING)


def test_a_blueprint_is_built_without_naming_a_crossing():
    assert isinstance(blueprint(), rrb.Blueprint)


def test_every_path_a_replay_draws_falls_under_a_view(store):
    reader = Store(store)
    crossings = reader.crossings(country=COUNTRY)
    steps = replay(CrowFlies(crossings), reader.samples(SESSION_ID))

    drawn = {drawing.path for drawing in drawings(steps, crossings, reader.passings(SESSION_ID))}

    assert drawn, "a replay of the fixture draws something"
    for path in drawn:
        assert any(path == prefix or path.startswith(f"{prefix}/") for prefix in VIEWED), (
            f"{path} is drawn but no view shows it"
        )
