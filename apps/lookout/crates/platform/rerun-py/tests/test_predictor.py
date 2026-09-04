"""What the runner gets when it drives the predictor from python.

The prediction itself is covered by the predictor's own tests. What only these can check is
the crossing: that an instant survives as an aware datetime, that a field left out stays
unknown rather than becoming zero, and that a mistake on this side raises rather than passes.
"""

import datetime

import pytest

from lookout_predictor import DEFAULT_RADIUS_METRES, CrowFlies, Prediction, Trend

# Three crossings due north of 50.0N, a hundredth of a degree apart, so the nearest is about
# 1,112m away and the furthest about 3,336m.
CROSSINGS = [(1, 50.01, 0.0), (2, 50.02, 0.0), (3, 50.03, 0.0)]
HUNDREDTH_DEGREE_M = 1111.95

T0 = datetime.datetime(2026, 7, 25, 9, 30, tzinfo=datetime.UTC)


def seconds(after):
    return T0 + datetime.timedelta(seconds=after)


@pytest.fixture
def predictor():
    return CrowFlies(CROSSINGS)


def test_a_predictor_predicts_nothing_before_its_first_fix(predictor):
    assert predictor.predictions() == []
    assert predictor.now is None


def test_a_fix_predicts_every_crossing_inside_the_radius_nearest_first(predictor):
    predictor.observe_sample(T0, 50.0, 0.0)

    predicted = predictor.predictions()

    assert [prediction.crossing for prediction in predicted] == [1, 2, 3]
    assert predicted[0].metres == pytest.approx(HUNDREDTH_DEGREE_M, abs=10.0)
    assert isinstance(predicted[0], Prediction)


def test_the_radius_is_the_callers_to_choose():
    predictor = CrowFlies(CROSSINGS, radius_metres=2_000.0)

    predictor.observe_sample(T0, 50.0, 0.0)

    assert [prediction.crossing for prediction in predictor.predictions()] == [1]
    assert DEFAULT_RADIUS_METRES == 5_000.0


def test_an_arrival_crosses_as_an_aware_instant(predictor):
    predictor.observe_sample(T0, 50.0, 0.0, speed_mps=10.0)

    at = predictor.predictions()[0].at

    assert at.tzinfo is not None
    assert at.utcoffset() == datetime.timedelta(0)
    assert (at - T0).total_seconds() == pytest.approx(HUNDREDTH_DEGREE_M / 10.0, abs=1.0)


def test_a_fix_advances_the_clock_to_its_own_instant(predictor):
    predictor.observe_sample(seconds(30), 50.0, 0.0)

    assert predictor.now == seconds(30)


def test_time_passing_advances_the_clock_and_leaves_the_prediction_alone(predictor):
    predictor.observe_sample(T0, 50.0, 0.0, speed_mps=10.0)
    predicted = predictor.predictions()

    predictor.observe_elapsed(seconds(60))

    assert predictor.now == seconds(60)
    assert predictor.predictions() == predicted


def test_a_speed_left_out_is_derived_from_the_fix_before(predictor):
    """A source reporting no speed still moves, and two fixes say how fast: a hundredth of a
    degree in a hundred seconds is about 11m/s, so the crossing 1,112m ahead is 100s away."""
    predictor.observe_sample(T0, 49.99, 0.0)
    assert predictor.predictions()[0].at is None, "nothing to derive a speed from yet"

    predictor.observe_sample(seconds(100), 50.0, 0.0)

    at = predictor.predictions()[0].at
    assert (at - seconds(100)).total_seconds() == pytest.approx(100.0, abs=1.0)


def test_a_standstill_predicts_a_distance_and_no_time(predictor):
    predictor.observe_sample(T0, 50.0, 0.0, speed_mps=0.0)

    predicted = predictor.predictions()[0]

    assert predicted.metres == pytest.approx(HUNDREDTH_DEGREE_M, abs=10.0)
    assert predicted.at is None


def test_a_crossing_trends_against_the_fix_before(predictor):
    """Running north from 50.0 to 50.03: the crossing at 50.02 is nearer than it was, and the
    one at 50.01 is now further behind us than it was ahead."""
    assert predictor.trend(1) is None

    predictor.observe_sample(T0, 50.0, 0.0)
    predictor.observe_sample(seconds(10), 50.03, 0.0)

    assert predictor.trend(2) == Trend.Closing
    assert predictor.trend(1) == Trend.Receding
    assert predictor.trend(99) is None, "never predicted, so nothing to compare"


def test_a_fix_behind_the_clock_is_refused_and_changes_nothing(predictor):
    predictor.observe_sample(seconds(30), 50.0, 0.0)
    predicted = predictor.predictions()

    with pytest.raises(ValueError, match="behind the clock"):
        predictor.observe_sample(T0, 50.02, 0.0)

    assert predictor.predictions() == predicted
    assert predictor.now == seconds(30)


def test_a_coordinate_off_the_globe_is_refused(predictor):
    with pytest.raises(ValueError, match="latitude"):
        predictor.observe_sample(T0, 91.0, 0.0)

    with pytest.raises(ValueError, match="longitude"):
        CrowFlies([(1, 50.0, 181.0)])


def test_a_naive_instant_is_refused(predictor):
    """The store's samples are tz-aware, and a naive one would be read as some other moment."""
    with pytest.raises(TypeError):
        predictor.observe_sample(T0.replace(tzinfo=None), 50.0, 0.0)
