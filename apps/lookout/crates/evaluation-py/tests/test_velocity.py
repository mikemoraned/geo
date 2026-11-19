import datetime

import pyarrow as pa
import pytest

import lookout_evaluation

START = datetime.datetime(2026, 11, 16, 12, tzinfo=datetime.UTC)


def samples(session_lats):
    rows = [
        (session_id, START + datetime.timedelta(seconds=10 * n), n, 0.0, lat)
        for session_id, lats in session_lats.items()
        for n, lat in enumerate(lats)
    ]
    session_ids, ts, seqs, lons, lats = zip(*rows)
    return pa.table(
        {
            "session_id": pa.array(session_ids, pa.string()),
            "t": pa.array(ts, pa.timestamp("ms", tz="UTC")),
            "seq": pa.array(seqs, pa.uint32()),
            "lon": pa.array(lons, pa.float64()),
            "lat": pa.array(lats, pa.float64()),
        }
    )


def estimated(session_lats):
    return pa.table(
        lookout_evaluation.velocities(samples=samples(session_lats), still_within_metres=20.0)
    ).to_pylist()


def test_each_sample_gets_the_velocity_estimated_once_it_is_observed():
    rows = estimated({"session-a": [50.0, 50.01, 50.01]})

    assert [(row["session_id"], row["t"]) for row in rows] == [
        ("session-a", START),
        ("session-a", START + datetime.timedelta(seconds=10)),
        ("session-a", START + datetime.timedelta(seconds=20)),
    ]
    assert [
        (round(row["speed_mps"], 1), round(row["heading_degrees"], 1))
        if row["speed_mps"] is not None
        else None
        for row in rows
    ] == [None, (111.2, 0.0), (55.6, 0.0)]


def test_each_session_starts_its_estimate_afresh():
    rows = estimated({"session-a": [50.0, 50.01], "session-b": [50.01]})

    assert [(row["session_id"], row["speed_mps"] is None) for row in rows] == [
        ("session-a", True),
        ("session-a", False),
        ("session-b", True),
    ]


def test_a_radius_of_stillness_below_zero_is_refused():
    with pytest.raises(ValueError):
        lookout_evaluation.velocities(
            samples=samples({"session-a": [50.0]}), still_within_metres=-1.0
        )
