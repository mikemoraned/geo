"""What a replay draws, read back from the recording it drew into."""

import datetime

from lookout_predictor import CrowFlies

from runner.log import (
    ACTUAL_ETA,
    CROSSINGS,
    DISTANCE,
    ERROR,
    FIX,
    PASSED,
    PASSING,
    PREDICTED_ETA,
    TRACK,
    draw,
    error_seconds,
)
from runner.replay import replay
from runner.store import Store

from conftest import COUNTRY, FAR, LON, NEAR, PASSED_AT, SESSION, T0, drawn, values


def replayed(store, recording, **kwargs):
    """The fixture session replayed into `recording`."""
    reader = Store(store)
    crossings = reader.crossings(country=COUNTRY)
    steps = replay(CrowFlies(crossings, **kwargs), reader.samples(SESSION))
    draw(recording, steps, crossings, reader.passings(SESSION))


class TestTheError:
    def test_a_prediction_later_than_the_passing_is_positive(self):
        passed = datetime.datetime(2026, 7, 26, 9, tzinfo=datetime.UTC)

        assert error_seconds(passed + datetime.timedelta(seconds=30), passed) == 30.0

    def test_a_prediction_earlier_than_the_passing_is_negative(self):
        passed = datetime.datetime(2026, 7, 26, 9, tzinfo=datetime.UTC)

        assert error_seconds(passed - datetime.timedelta(seconds=12.5), passed) == -12.5

    def test_a_prediction_with_no_time_has_no_error(self):
        assert error_seconds(None, datetime.datetime.now(datetime.UTC)) is None

    def test_a_crossing_the_session_never_passed_has_no_error(self):
        assert error_seconds(datetime.datetime.now(datetime.UTC), None) is None


class TestTheMap:
    def test_the_crossings_are_drawn_once_for_the_whole_recording(self, store, recording):
        replayed(store, recording)
        logged = drawn(recording, CROSSINGS)

        assert len(logged) == 1
        assert logged[0].at is None, "the map is not a moment in the run"
        assert sorted(values(logged[0].archetype)) == [[50.04, LON], [50.06, LON]]

    def test_each_fix_is_drawn_at_its_own_instant(self, store, recording):
        replayed(store, recording)
        logged = drawn(recording, FIX)

        assert [values(one.archetype)[0][0] for one in logged] == [50.0, 50.01, 50.02, 50.03]
        assert [one.at for one in logged] == sorted(one.at for one in logged)
        assert logged[0].at == T0

    def test_the_track_is_the_whole_run(self, store, recording):
        replayed(store, recording)
        logged = drawn(recording, TRACK)

        assert len(logged) == 1
        assert logged[0].at is None
        assert len(values(logged[0].archetype)[0]) == 4, "a vertex per fix"


class TestThePredictions:
    def test_a_crossing_carries_its_distance_at_every_fix(self, store, recording):
        replayed(store, recording)
        metres = [values(one.archetype)[0] for one in drawn(recording, f"{DISTANCE}/{NEAR}")]

        assert len(metres) == 4, "the near crossing is inside the radius from the first fix"
        assert metres == sorted(metres, reverse=True), "closing as the run goes north"

    def test_the_error_is_a_series_rather_than_one_number_at_the_end(self, store, recording):
        replayed(store, recording)
        logged = drawn(recording, f"{ERROR}/{NEAR}")

        # The second fix onwards predicts a time; the first has no speed to divide by.
        assert len(logged) == 3
        assert [one.at for one in logged] == sorted(one.at for one in logged)
        assert all(abs(values(one.archetype)[0]) < 5.0 for one in logged), "close, on a straight run"

    def test_a_crossing_the_session_never_passed_is_drawn_without_an_error(self, store, recording):
        replayed(store, recording)

        assert drawn(recording, f"{DISTANCE}/{FAR}")
        assert drawn(recording, f"{ERROR}/{FAR}") == []

    def test_the_radius_bounds_what_is_drawn(self, store, recording):
        replayed(store, recording, radius_metres=3_000.0)

        assert drawn(recording, f"{DISTANCE}/{FAR}") == [], "the far crossing stays out of range"
        assert len(drawn(recording, f"{DISTANCE}/{NEAR}")) == 2, "the near one only at the end"


class TestTheGroundTruth:
    def test_the_crossings_the_session_reached_are_drawn_apart_from_the_rest(
        self, store, recording
    ):
        replayed(store, recording)
        logged = drawn(recording, PASSED)

        assert len(logged) == 1
        assert logged[0].at is None
        assert values(logged[0].archetype) == [[50.04, LON]], "the near crossing, not the far one"

    def test_each_passing_is_noted_at_the_moment_it_happened(self, store, recording):
        replayed(store, recording)
        logged = drawn(recording, f"{PASSING}/{NEAR}")

        assert len(logged) == 1
        assert logged[0].at == PASSED_AT
        assert "8m away" in values(logged[0].archetype)[0], "how good the evidence for it is"

    def test_the_true_countdown_is_drawn_against_the_predicted_one(self, store, recording):
        replayed(store, recording)
        predicted = drawn(recording, f"{PREDICTED_ETA}/{NEAR}")
        actual = drawn(recording, f"{ACTUAL_ETA}/{NEAR}")

        assert [one.at for one in predicted] == [one.at for one in actual][1:]
        countdown = [values(one.archetype)[0] for one in actual]
        assert countdown == sorted(countdown, reverse=True), "counting down to the passing"
        assert countdown[-1] == 60.0, "a minute after the last fix"

    def test_a_crossing_the_session_never_reached_has_no_countdown_to_answer_to(
        self, store, recording
    ):
        replayed(store, recording)

        assert drawn(recording, f"{PREDICTED_ETA}/{FAR}")
        assert drawn(recording, f"{ACTUAL_ETA}/{FAR}") == []


def test_a_replay_of_nothing_still_draws_the_map(store, recording):
    reader = Store(store)

    draw(recording, [], reader.crossings(country=COUNTRY), [])

    assert {one.path for one in drawn(recording)} == {CROSSINGS, PASSED, TRACK}
    assert values(drawn(recording, TRACK)[0].archetype) == [[]], "a track of no fixes"
