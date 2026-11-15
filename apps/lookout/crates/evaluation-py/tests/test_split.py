import pyarrow as pa
import pytest

import lookout_evaluation

DRESDEN_HBF = (13.7322, 51.0403)
SOUTH_OF_DRESDEN = (13.7322, 50.7403)


def splitter():
    return lookout_evaluation.Splitter(resolution=8, salt="tune-test-1", tune_buckets=70)


def splits(points):
    lon = pa.array([lon for lon, _ in points], pa.float64())
    lat = pa.array([lat for _, lat in points], pa.float64())
    return pa.array(splitter().splits(lon, lat)).to_pylist()


def test_dresden_hbf_lies_in_the_test_split_and_a_point_south_of_it_in_the_tune_split():
    assert splits([DRESDEN_HBF, SOUTH_OF_DRESDEN]) == ["test", "tune"]


def test_points_of_two_lengths_are_refused():
    with pytest.raises(ValueError, match="lon"):
        splitter().splits(pa.array([1.0, 2.0]), pa.array([1.0]))


def test_points_holding_a_null_are_refused():
    with pytest.raises(ValueError, match="nulls"):
        splitter().splits(pa.array([1.0, None]), pa.array([1.0, 2.0]))
