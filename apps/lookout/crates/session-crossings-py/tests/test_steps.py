import math
from datetime import UTC, datetime, timedelta

import pyarrow as pa
import pytest
import shapely

import lookout_session_crossings

ORIGIN = (500_000.0, 5_800_000.0)
START = datetime(2026, 7, 22, 15, 9, tzinfo=UTC)


def at(minute):
    return START + timedelta(minutes=minute)


def samples(*fixes):
    return pa.table(
        {
            "t": pa.array([at(minute) for minute, _ in fixes], pa.timestamp("ms", "UTC")),
            "geometry_projected": shapely.to_wkb(
                shapely.points([(ORIGIN[0] + east, ORIGIN[1]) for _, east in fixes])
            ),
        }
    )


def crossings(*placed):
    return pa.table(
        {
            "crossing_id": [crossing_id for crossing_id, _ in placed],
            "geometry": shapely.to_wkb(shapely.points([(8.65, 50.1)] * len(placed))),
            "geometry_projected": shapely.to_wkb(
                shapely.points([(ORIGIN[0] + east, ORIGIN[1]) for _, east in placed])
            ),
        }
    )


def steps(fixes, placed, radius_metres=50.0):
    return lookout_session_crossings.steps(
        "session-a", fixes, placed, radius_metres=radius_metres
    )


def test_a_session_reversing_over_a_crossing_passes_it_twice():
    passes = pa.table(
        steps(
            samples((0, 0.0), (1, 200.0), (2, 400.0), (3, 200.0), (4, 0.0)),
            crossings(("c", 200.0)),
        )["passes"]
    )

    assert passes.column("crossed_at").to_pylist() == [at(1), at(3)]
    assert passes.column("crossing_id").to_pylist() == ["c", "c"]


def test_each_near_sample_is_numbered_by_its_visit():
    near = pa.table(
        steps(
            samples((0, 0.0), (1, 190.0), (2, 210.0), (3, 400.0), (4, 200.0)),
            crossings(("c", 200.0)),
        )["near"]
    )

    assert near.column("t").to_pylist() == [at(1), at(2), at(4)]
    assert near.column("visit").to_pylist() == [0, 0, 1]
    assert near.column("distance_m").to_pylist() == pytest.approx([10.0, 10.0, 0.0])


def test_samples_are_matched_in_time_order_whatever_order_they_arrive_in():
    passes = pa.table(
        steps(
            samples((3, 200.0), (0, 0.0), (2, 400.0), (1, 200.0)),
            crossings(("c", 200.0)),
        )["passes"]
    )

    assert passes.column("crossed_at").to_pylist() == [at(1), at(3)]


def test_each_crossing_is_matched_in_one_call():
    passes = pa.table(
        steps(
            samples((0, 0.0), (1, 200.0), (2, 400.0)),
            crossings(("first", 0.0), ("second", 400.0)),
        )["passes"]
    )

    assert sorted(passes.column("crossing_id").to_pylist()) == ["first", "second"]
    assert passes.column("samples_within").to_pylist() == [1, 1]


def test_a_table_missing_a_column_is_refused():
    with pytest.raises(ValueError, match="geometry_projected"):
        steps(
            samples((0, 0.0)).drop_columns(["geometry_projected"]),
            crossings(("c", 0.0)),
        )


@pytest.mark.parametrize("radius_metres", [0.0, -50.0, math.inf, math.nan])
def test_a_radius_that_is_not_positive_and_finite_is_refused(radius_metres):
    with pytest.raises(ValueError, match="radius"):
        steps(samples((0, 0.0)), crossings(("c", 0.0)), radius_metres=radius_metres)
