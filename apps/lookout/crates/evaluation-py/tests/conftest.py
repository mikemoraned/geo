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


def train_session_table(session_ids, realtime_percentages, day="2026-10-18"):
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
            "ended_at": started,
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
