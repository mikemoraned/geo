"""A store to read, and a stand-in for the recording a replay draws into.

Written rather than laid out by hand, so what these tests read back is a real dataset: the
columns `crates/model` declares, the partitions the store chose, and geometry with the CRS
the file states. A schema change lands here as a write that is refused, which is the point.
"""

import datetime
from unittest.mock import create_autospec

import lookout_medallion
import pyarrow as pa
import pyproj
import pytest
import rerun as rr
import shapely

SESSION = "1e1b4a2c-0000-4000-8000-000000000001"
DEVICE = "d0000000-0000-4000-8000-000000000001"
COUNTRY = "DE"

# A run north up the 8.6E meridian at a hundredth of a degree a minute, starting the minute
# before midnight so its samples fall in two date partitions.
T0 = datetime.datetime(2026, 7, 25, 23, 58, tzinfo=datetime.UTC)
LON = 8.6
START_LAT = 50.0
# Two crossings ahead of the run: the nearer about 3.9km from the first fix and about 560m
# from the last, so it comes inside the radius a line is drawn within; the further about
# 6.7km, which never does.
NEAR, FAR = 0x292E417A, 0x51B0C33D
NEAR_LAT, FAR_LAT = 50.035, 50.06


def _projected(points):
    """`points` in the zone the store projects this country into."""
    transformer = pyproj.Transformer.from_crs(
        "EPSG:4326", lookout_medallion.projected_crs(COUNTRY), always_xy=True
    )
    return [shapely.Point(transformer.transform(lon, lat)) for lon, lat in points]


def _wkb(points):
    return pa.array([shapely.to_wkb(shapely.Point(point)) for point in points], pa.binary())


def _sample_table():
    minutes = range(4)
    instants = [T0 + datetime.timedelta(minutes=minute) for minute in minutes]
    points = [(LON, START_LAT + minute / 100.0) for minute in minutes]
    # The first sample reports no speed and no altitude, as a device does before it has a
    # fix good enough to derive them from.
    absent_at_first = [None if minute == 0 else 18.5 for minute in minutes]

    return pa.table(
        {
            "session_id": pa.array([SESSION] * len(instants), pa.string()),
            "device_id": pa.array([DEVICE] * len(instants), pa.string()),
            "t": pa.array(instants, pa.timestamp("ms", tz="UTC")),
            "seq": pa.array(list(minutes), pa.uint32()),
            "lat": pa.array([lat for _, lat in points], pa.float64()),
            "lon": pa.array([lon for lon, _ in points], pa.float64()),
            "alt": pa.array(
                [None if minute == 0 else 12.5 for minute in minutes], pa.float64()
            ),
            "acc": pa.array([4.8] * len(instants), pa.float64()),
            "speed": pa.array(absent_at_first, pa.float64()),
            "heading": pa.array([0.0] * len(instants), pa.float64()),
            "implied_speed_mps": pa.array([None] * len(instants), pa.float64()),
            "geometry": _wkb(points),
            "geometry_projected": pa.array(
                [shapely.to_wkb(point) for point in _projected(points)], pa.binary()
            ),
            "country": pa.array([COUNTRY] * len(instants), pa.string()),
            "sample_date": pa.array(
                [instant.date() for instant in instants], pa.date32()
            ),
        }
    )


def _crossing_table():
    points = [(LON, NEAR_LAT), (LON, FAR_LAT)]
    rows = len(points)
    return pa.table(
        {
            "crossing_id": pa.array(["w1-t1", "w2-t1"], pa.string()),
            "crossing_short_id": pa.array([NEAR, FAR], pa.uint32()),
            "water_id": pa.array(["water-1", "water-2"], pa.string()),
            "water_subtype": pa.array(["river"] * rows, pa.string()),
            "water_class": pa.array(["river"] * rows, pa.string()),
            "track_id": pa.array(["track-1"] * rows, pa.string()),
            "rail_id": pa.array(["rail-1"] * rows, pa.string()),
            "rail_class": pa.array(["rail"] * rows, pa.string()),
            "overlap_kind": pa.array(["point"] * rows, pa.string()),
            "overlap_m": pa.array([0.0] * rows, pa.float64()),
            "total_overlap_m": pa.array([0.0] * rows, pa.float64()),
            "merged_parts": pa.array([1] * rows, pa.uint32()),
            "frac": pa.array([0.5] * rows, pa.float64()),
            "extract_id": pa.array(["20260727T193628Z"] * rows, pa.string()),
            "merge_distance_m": pa.array([25.0] * rows, pa.float64()),
            "min_crossing_m": pa.array([5.0] * rows, pa.float64()),
            "geometry": _wkb(points),
            "geometry_projected": pa.array(
                [shapely.to_wkb(point) for point in _projected(points)], pa.binary()
            ),
            "country": pa.array([COUNTRY] * rows, pa.string()),
        }
    )


@pytest.fixture
def empty_store(tmp_path):
    """A store with nothing derived into it yet."""
    return tmp_path


@pytest.fixture
def store(tmp_path):
    """One session that crosses midnight, and the two crossings ahead of it."""
    lookout_medallion.write_silver("session_sample", _sample_table(), root=str(tmp_path))
    lookout_medallion.write_silver("water_crossing", _crossing_table(), root=str(tmp_path))
    return tmp_path


@pytest.fixture
def recording():
    """A stand-in for the recording a replay draws into.

    Checked against the real `RecordingStream`, so a call this does not object to is one
    rerun would also have accepted — a renamed method or a changed signature fails here
    rather than passing against a hand-written double of an API that has moved on.
    """
    return create_autospec(rr.RecordingStream, instance=True)


def streams(recording) -> set[str]:
    """The streams drawn to, taken from the entity path each log named."""
    return {call.args[0] for call in recording.log.call_args_list}
