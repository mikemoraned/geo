"""Feeding a session through a predictor."""

from lookout_predictor import CrowFlies

from runner.replay import replay
from runner.store import Store

from conftest import COUNTRY, FAR, NEAR, SESSION


def test_every_sample_is_a_step_carrying_what_was_predicted_from_it(store):
    reader = Store(store)

    steps = list(
        replay(CrowFlies(reader.crossings(country=COUNTRY)), reader.samples(SESSION))
    )

    assert len(steps) == 4
    assert [step.sample.lat for step in steps] == [50.0, 50.01, 50.02, 50.03]
    # The further crossing starts outside the default 5km radius and comes into view as the
    # run closes on it.
    assert [prediction.crossing_compact_id for prediction in steps[0].predictions] == [NEAR]
    assert [prediction.crossing_compact_id for prediction in steps[-1].predictions] == [NEAR, FAR]


def test_a_crossing_ahead_draws_nearer_as_the_session_runs(store):
    reader = Store(store)

    steps = list(
        replay(CrowFlies(reader.crossings(country=COUNTRY)), reader.samples(SESSION))
    )

    metres = [step.predictions[0].metres for step in steps]
    assert metres == sorted(metres, reverse=True)
    assert metres[0] > 3_000.0 and metres[-1] < 1_000.0


def test_the_first_fix_predicts_no_time_and_the_rest_do(store):
    """The first sample of this session reports no speed and has no fix before it to derive
    one from, so it says how far but not when. The rest report one."""
    reader = Store(store)

    steps = list(
        replay(CrowFlies(reader.crossings(country=COUNTRY)), reader.samples(SESSION))
    )

    assert steps[0].predictions[0].at is None
    assert all(step.predictions[0].at > step.sample.t for step in steps[1:])


def test_the_radius_bounds_what_a_step_carries(store):
    reader = Store(store)

    steps = list(
        replay(
            CrowFlies(reader.crossings(country=COUNTRY), radius_metres=3_000.0),
            reader.samples(SESSION),
        )
    )

    assert steps[0].predictions == [], "both crossings start outside the radius"
    assert [prediction.crossing_compact_id for prediction in steps[-1].predictions] == [NEAR]


def test_a_session_with_no_samples_replays_as_nothing(store):
    assert list(replay(CrowFlies(Store(store).crossings()), [])) == []
