"""What a replay draws, and what drawing it produces."""

import datetime

import rerun as rr
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
    Line,
    Marker,
    Measure,
    Note,
    draw,
    drawings,
    error_seconds,
)
from runner.replay import replay
from runner.store import Store

from conftest import COUNTRY, FAR, LON, NEAR, PASSED_AT, SESSION


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


def drawn(store, **kwargs):
    reader = Store(store)
    crossings = reader.crossings(country=COUNTRY)
    steps = replay(CrowFlies(crossings, **kwargs), reader.samples(SESSION))
    return list(drawings(steps, crossings, reader.passings(SESSION)))


def paths(drawings_, kind=None):
    return [
        drawing.path
        for drawing in drawings_
        if kind is None or isinstance(drawing, kind)
    ]


class TestWhatAReplayDraws:
    def test_the_crossings_are_drawn_once_for_the_whole_recording(self, store):
        marks = [d for d in drawn(store) if d.path == CROSSINGS]

        assert len(marks) == 1
        assert marks[0].at is None, "the map is not a moment in the run"
        assert sorted(marks[0].lat_lon) == [(50.04, LON), (50.06, LON)]

    def test_each_fix_is_drawn_at_its_own_instant(self, store):
        fixes = [d for d in drawn(store) if d.path == FIX]

        assert [mark.lat_lon[0][0] for mark in fixes] == [50.0, 50.01, 50.02, 50.03]
        assert [mark.at for mark in fixes] == sorted(mark.at for mark in fixes)

    def test_the_track_is_the_whole_run_drawn_last(self, store):
        drawings_ = drawn(store)
        track = drawings_[-1]

        assert isinstance(track, Line)
        assert track.path == TRACK
        assert track.at is None
        assert len(track.lat_lon) == 4

    def test_a_predicted_crossing_carries_its_distance_at_every_fix(self, store):
        metres = [d for d in drawn(store) if d.path == f"{DISTANCE}/{NEAR}"]

        assert len(metres) == 4, "the near crossing is inside the radius from the first fix"
        assert [measure.value for measure in metres] == sorted(
            (measure.value for measure in metres), reverse=True
        ), "closing as the run goes north"

    def test_the_error_is_a_series_against_the_passing_the_store_recorded(self, store):
        errors = [d for d in drawn(store) if d.path == f"{ERROR}/{NEAR}"]

        assert [measure.at for measure in errors] == sorted(
            measure.at for measure in errors
        )
        assert all(isinstance(measure, Measure) for measure in errors)
        # The second fix onwards predicts a time; the first has no speed to divide by.
        assert len(errors) == 3

    def test_a_crossing_the_session_never_passed_is_drawn_without_an_error(self, store):
        assert paths(drawn(store), Measure).count(f"{DISTANCE}/{FAR}") > 0
        assert f"{ERROR}/{FAR}" not in paths(drawn(store))

    def test_a_replay_of_nothing_still_draws_the_map_and_an_empty_track(self, store):
        reader = Store(store)
        crossings = reader.crossings(country=COUNTRY)

        drawings_ = list(drawings([], crossings, []))

        assert paths(drawings_) == [CROSSINGS, PASSED, TRACK]
        assert drawings_[-1].lat_lon == []


class TestTheGroundTruth:
    def test_the_crossings_the_session_reached_are_drawn_apart_from_the_rest(self, store):
        passed = [d for d in drawn(store) if d.path == PASSED]

        assert len(passed) == 1
        assert passed[0].at is None
        assert passed[0].lat_lon == [(50.04, LON)], "the near crossing, not the far one"

    def test_each_passing_is_noted_at_the_moment_it_happened(self, store):
        notes = [d for d in drawn(store) if isinstance(d, Note)]

        assert [note.path for note in notes] == [f"{PASSING}/{NEAR}"]
        assert notes[0].at == PASSED_AT
        assert "8m away" in notes[0].text, "how good the evidence for it is"

    def test_the_true_countdown_is_drawn_against_the_predicted_one(self, store):
        drawings_ = drawn(store)
        predicted = [d for d in drawings_ if d.path == f"{PREDICTED_ETA}/{NEAR}"]
        actual = [d for d in drawings_ if d.path == f"{ACTUAL_ETA}/{NEAR}"]

        assert [measure.at for measure in predicted] == [measure.at for measure in actual][1:]
        assert [measure.value for measure in actual] == sorted(
            (measure.value for measure in actual), reverse=True
        ), "counting down to the moment it was passed"
        assert actual[-1].value == 60.0, "a minute after the last fix"

    def test_a_crossing_the_session_never_reached_has_no_countdown_to_answer_to(self, store):
        drawings_ = drawn(store)

        assert [d for d in drawings_ if d.path == f"{PREDICTED_ETA}/{FAR}"]
        assert not [d for d in drawings_ if d.path == f"{ACTUAL_ETA}/{FAR}"]


def test_drawing_logs_every_drawing_to_the_recording(store):
    recording = rr.RecordingStream("test")
    memory = recording.memory_recording()

    draw(
        recording,
        [
            Marker(CROSSINGS, [(50.0, 8.6)]),
            Marker(FIX, [(50.0, 8.6)], PASSED_AT),
            Measure("predicted/1/metres", 1234.5, PASSED_AT),
            Line(TRACK, [(50.0, 8.6), (50.01, 8.6)]),
        ],
    )

    assert memory.num_msgs() >= 4
