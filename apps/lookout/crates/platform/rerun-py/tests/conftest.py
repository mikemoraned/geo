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

ELSEWHERE_SESSION = "1e1b4a2c-0000-4000-8000-000000000002"
ELSEWHERE_DEVICE = "d0000000-0000-4000-8000-000000000002"
ELSEWHERE = "GB"

JUST_BEFORE_MIDNIGHT = datetime.datetime(2026, 7, 25, 23, 58, tzinfo=datetime.UTC)
AN_HOUR_EARLIER = JUST_BEFORE_MIDNIGHT - datetime.timedelta(hours=1)
LON = 8.6
START_LAT = 50.0
ELSEWHERE_LON = -3.19
ELSEWHERE_START_LAT = 55.95
A_HUNDREDTH_OF_A_DEGREE_A_MINUTE = 1 / 100.0

NEAR, FAR = 0x292E417A, 0x51B0C33D
COMES_INSIDE_THE_RADIUS, STAYS_OUTSIDE_IT = 50.035, 50.06


def _projected(points, country):
    transformer = pyproj.Transformer.from_crs(
        "EPSG:4326", lookout_medallion.projected_crs(country), always_xy=True
    )
    return [shapely.Point(transformer.transform(lon, lat)) for lon, lat in points]


def _wkb(points):
    return pa.array([shapely.to_wkb(shapely.Point(point)) for point in points], pa.binary())


def _sample_table(session_id, device_id, country, lon, start_lat, first):
    minutes = range(4)
    instants = [first + datetime.timedelta(minutes=minute) for minute in minutes]
    points = [
        (lon, start_lat + minute * A_HUNDREDTH_OF_A_DEGREE_A_MINUTE) for minute in minutes
    ]
    speed_absent_until_a_fix_can_derive_it = [
        None if minute == 0 else 18.5 for minute in minutes
    ]

    return pa.table(
        {
            "session_id": pa.array([session_id] * len(instants), pa.string()),
            "device_id": pa.array([device_id] * len(instants), pa.string()),
            "t": pa.array(instants, pa.timestamp("ms", tz="UTC")),
            "seq": pa.array(list(minutes), pa.uint32()),
            "lat": pa.array([lat for _, lat in points], pa.float64()),
            "lon": pa.array([lon for lon, _ in points], pa.float64()),
            "alt": pa.array(
                [None if minute == 0 else 12.5 for minute in minutes], pa.float64()
            ),
            "acc": pa.array([4.8] * len(instants), pa.float64()),
            "speed": pa.array(speed_absent_until_a_fix_can_derive_it, pa.float64()),
            "heading": pa.array([0.0] * len(instants), pa.float64()),
            "implied_speed_mps": pa.array([None] * len(instants), pa.float64()),
            "geometry": _wkb(points),
            "geometry_projected": pa.array(
                [shapely.to_wkb(point) for point in _projected(points, country)],
                pa.binary(),
            ),
            "country": pa.array([country] * len(instants), pa.string()),
            "sample_date": pa.array(
                [instant.date() for instant in instants], pa.date32()
            ),
        }
    )


def _crossing_table():
    points = [(LON, COMES_INSIDE_THE_RADIUS), (LON, STAYS_OUTSIDE_IT)]
    rows = len(points)
    return pa.table(
        {
            "crossing_id": pa.array(["w1-t1", "w2-t1"], pa.string()),
            "crossing_compact_id": pa.array([NEAR, FAR], pa.uint32()),
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
                [shapely.to_wkb(point) for point in _projected(points, COUNTRY)], pa.binary()
            ),
            "country": pa.array([COUNTRY] * rows, pa.string()),
        }
    )


@pytest.fixture
def empty_store(tmp_path):
    return tmp_path


def _samples_of_every_country():
    return pa.concat_tables(
        [
            _sample_table(
                SESSION, DEVICE, COUNTRY, LON, START_LAT, JUST_BEFORE_MIDNIGHT
            ),
            _sample_table(
                ELSEWHERE_SESSION,
                ELSEWHERE_DEVICE,
                ELSEWHERE,
                ELSEWHERE_LON,
                ELSEWHERE_START_LAT,
                AN_HOUR_EARLIER,
            ),
        ]
    )


@pytest.fixture
def store(tmp_path):
    lookout_medallion.write_silver(
        "session_sample", _samples_of_every_country(), root=str(tmp_path)
    )
    lookout_medallion.write_silver("water_crossing", _crossing_table(), root=str(tmp_path))
    return tmp_path


@pytest.fixture
def recording():
    return create_autospec(rr.RecordingStream, instance=True)


def streams(recording) -> set[str]:
    return {call.args[0] for call in recording.log.call_args_list}
