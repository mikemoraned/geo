import datetime

import pyarrow as pa
import pytest
import shapely

BERLIN = (13.404954, 52.520008)
FRANKFURT = (8.682127, 50.110924)
BERLIN_UTM32N = (798809.63, 5828000.60)
FRANKFURT_UTM32N = (477271.45, 5551012.24)
SOURCE_ID = "fe022bac-a65e-5a96-8e66-377dd523c2ac"
CORNER = pa.struct(
    [pa.field(name, pa.float64(), nullable=False) for name in ("xmin", "ymin", "xmax", "ymax")]
)


@pytest.fixture
def store(tmp_path):
    return tmp_path


def train_session_table(session_ids, realtime_percentages, day="2026-10-18", lasting=0):
    rows = len(session_ids)
    line = shapely.LineString([BERLIN, FRANKFURT])
    projected = shapely.LineString([BERLIN_UTM32N, FRANKFURT_UTM32N])
    started = pa.array([f"{day}T09:00:00Z"] * rows, pa.string()).cast(
        pa.timestamp("ms", tz="UTC")
    )
    return pa.table(
        {
            "geometry": pa.array([shapely.to_wkb(line)] * rows, pa.binary()),
            "geometry_projected": pa.array([shapely.to_wkb(projected)] * rows, pa.binary()),
            "session_id": pa.array(session_ids, pa.string()),
            "source_id": pa.array([SOURCE_ID] * rows, pa.string()),
            "source_label": pa.array([None] * rows, pa.string()),
            "trip_id": pa.array([f"trip-{id}" for id in session_ids], pa.string()),
            "started_at": started,
            "ended_at": pa.array(
                [start_of(day) + datetime.timedelta(seconds=lasting)] * rows,
                pa.timestamp("ms", tz="UTC"),
            ),
            "sample_count": pa.array([1] * rows, pa.uint32()),
            "realtime_percentage": pa.array(realtime_percentages, pa.float64()),
            "bbox": pa.array(
                [
                    {
                        "xmin": FRANKFURT[0],
                        "ymin": FRANKFURT[1],
                        "xmax": BERLIN[0],
                        "ymax": BERLIN[1],
                    }
                ]
                * rows,
                CORNER,
            ),
            "country": pa.array(["DE"] * rows, pa.string()),
            "start_date": pa.array([day] * rows, pa.string()).cast(pa.date32()),
        }
    )



def start_of(day):
    return datetime.datetime.fromisoformat(f"{day}T09:00:00+00:00")


def _point_wkb(point, rows):
    return pa.array([shapely.to_wkb(shapely.Point(point))] * rows, pa.binary())


def train_sample_table(session_ids, seconds, day="2026-10-18"):
    rows = [(session_id, step) for session_id in session_ids for step in range(len(seconds))]
    count = len(rows)
    return pa.table(
        {
            "session_id": pa.array([session_id for session_id, _ in rows], pa.string()),
            "source_id": pa.array([SOURCE_ID] * count, pa.string()),
            "trip_id": pa.array([f"trip-{session_id}" for session_id, _ in rows], pa.string()),
            "t": pa.array(
                [start_of(day) + datetime.timedelta(seconds=seconds[step]) for _, step in rows],
                pa.timestamp("ms", tz="UTC"),
            ),
            "seq": pa.array([step for _, step in rows], pa.uint32()),
            "lat": pa.array([BERLIN[1]] * count, pa.float64()),
            "lon": pa.array([BERLIN[0]] * count, pa.float64()),
            "implied_speed_mps": pa.array([None] * count, pa.float64()),
            "geometry_source": pa.array(["interpolated"] * count, pa.string()),
            "realtime": pa.array([True] * count, pa.bool_()),
            "geometry": _point_wkb(BERLIN, count),
            "geometry_projected": _point_wkb(BERLIN_UTM32N, count),
            "country": pa.array(["DE"] * count, pa.string()),
            "sample_date": pa.array([day] * count, pa.string()).cast(pa.date32()),
        }
    )


def train_pass_table(passes, day="2026-10-18"):
    count = len(passes)
    return pa.table(
        {
            "session_id": pa.array([session_id for session_id, _, _ in passes], pa.string()),
            "crossing_id": pa.array([crossing_id for _, crossing_id, _ in passes], pa.string()),
            "crossed_at": pa.array(
                [start_of(day) + datetime.timedelta(seconds=seconds) for _, _, seconds in passes],
                pa.timestamp("ms", tz="UTC"),
            ),
            "distance_m": pa.array([0.0] * count, pa.float64()),
            "samples_within": pa.array([1] * count, pa.uint32()),
            "match_radius_m": pa.array([250.0] * count, pa.float64()),
            "crossed_date": pa.array([day] * count, pa.string()).cast(pa.date32()),
        }
    )


def crossing_table(crossings):
    count = len(crossings)
    return pa.table(
        {
            "crossing_id": pa.array([crossing_id for crossing_id, _ in crossings], pa.string()),
            "crossing_compact_id": pa.array([compact for _, compact in crossings], pa.uint32()),
            "overture_water_id": pa.array(["water"] * count, pa.string()),
            "overture_water_subtype": pa.array(["river"] * count, pa.string()),
            "overture_water_class": pa.array(["river"] * count, pa.string()),
            "track_id": pa.array(["track"] * count, pa.string()),
            "overture_rail_id": pa.array(["rail"] * count, pa.string()),
            "overture_rail_class": pa.array(["rail"] * count, pa.string()),
            "overlap_kind": pa.array(["point"] * count, pa.string()),
            "overlap_m": pa.array([0.0] * count, pa.float64()),
            "total_overlap_m": pa.array([0.0] * count, pa.float64()),
            "merged_parts": pa.array([1] * count, pa.uint32()),
            "frac": pa.array([0.5] * count, pa.float64()),
            "extract_id": pa.array(["20260727T193628Z"] * count, pa.string()),
            "merge_distance_m": pa.array([25.0] * count, pa.float64()),
            "min_crossing_m": pa.array([5.0] * count, pa.float64()),
            "geometry": _point_wkb(BERLIN, count),
            "geometry_projected": _point_wkb(BERLIN_UTM32N, count),
            "country": pa.array(["DE"] * count, pa.string()),
        }
    )
