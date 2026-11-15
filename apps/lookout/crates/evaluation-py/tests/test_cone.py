import datetime

import pyarrow as pa

import lookout_evaluation

START = datetime.datetime(2026, 11, 15, 12, tzinfo=datetime.UTC)
SOUTH_OF_DRESDEN = (13.7322, 50.7403)
EAST_110_METRES = 0.00156


def eastward(samples):
    lon, lat = SOUTH_OF_DRESDEN
    return pa.table(
        {
            "session_id": pa.array(["session-a"] * samples, pa.string()),
            "t": pa.array(
                [START + datetime.timedelta(seconds=10 * n) for n in range(samples)],
                pa.timestamp("ms", tz="UTC"),
            ),
            "seq": pa.array(range(samples), pa.uint32()),
            "lon": pa.array([lon + EAST_110_METRES * n for n in range(samples)], pa.float64()),
            "lat": pa.array([lat] * samples, pa.float64()),
        }
    )


def stages(samples, split="tune"):
    splitter = lookout_evaluation.Splitter(resolution=8, salt="tune-test-1", tune_buckets=70)
    return lookout_evaluation.cone_stages(
        samples=samples,
        split=lookout_evaluation.ScoredSplit(splitter=splitter, split=split),
        horizon="5m",
        dwelling_metres=500.0,
    )


def test_each_stage_of_a_straight_track_holds_its_rows():
    computed = stages(eastward(40))

    assert {
        name: pa.table(computed[name]).num_rows
        for name in ["steps", "pairs", "windows", "pair_angles", "window_angles"]
    } == {"steps": 40, "pairs": 705, "windows": 38, "pair_angles": 695, "window_angles": 34}


def test_a_straight_track_keeps_every_later_position_on_its_heading():
    angles = pa.table(stages(eastward(40))["window_angles"])

    assert angles.column_names == ["session_id", "t", "angle"]
    assert max(angles.column("angle").to_pylist()) < 0.1


def test_a_track_outside_the_split_measures_no_window():
    computed = stages(eastward(40), split="test")

    assert pa.table(computed["windows"]).num_rows == 38
    assert pa.table(computed["window_angles"]).num_rows == 0
