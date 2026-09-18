from lookout_predictor import CrowFlies

from runner.log import LOG, POSITION, POSITIONS, PREDICTIONS, draw
from runner.replay import replay
from runner.store import Store

from conftest import COUNTRY, SESSION, streams


def test_a_replay_draws_to_every_stream(store, recording):
    reader = Store(store)
    crossings = reader.crossings(country=COUNTRY)

    draw(recording, replay(CrowFlies(crossings), reader.samples(SESSION)), crossings)

    assert {LOG, POSITION, POSITIONS, PREDICTIONS} <= streams(recording)
