import pyarrow as pa
import pytest
import shapely

BERLIN = (13.404954, 52.520008)
FRANKFURT = (8.682127, 50.110924)
BERLIN_UTM32N = (798809.63, 5828000.60)
PROJECTED_CRS = "EPSG:25832"
FRANKFURT_UTM32N = (477271.45, 5551012.24)


@pytest.fixture
def store(tmp_path):
    return tmp_path


def train_segment_table(trip_ids, departures, countries):
    line = shapely.LineString([BERLIN, FRANKFURT])
    projected = shapely.LineString([BERLIN_UTM32N, FRANKFURT_UTM32N])
    rows = len(trip_ids)
    return pa.table(
        {
            "trip_id": pa.array(trip_ids, pa.string()),
            "route_name": pa.array(["ICE 123"] * rows, pa.string()),
            "train_number": pa.array([123] * rows, pa.uint32()),
            "agency_id": pa.array(["db"] * rows, pa.string()),
            "agency_name": pa.array(["DB"] * rows, pa.string()),
            "mode": pa.array(["HIGHSPEED_RAIL"] * rows, pa.string()),
            "route_color": pa.array(["ff0000"] * rows, pa.string()),
            "realtime": pa.array([True] * rows, pa.bool_()),
            "from_stop_id": pa.array(["berlin-hbf"] * rows, pa.string()),
            "departure": pa.array(
                [f"{day}T09:00:00Z" for day in departures], pa.string()
            ).cast(pa.timestamp("ms", tz="UTC")),
            "arrival": pa.array(
                [f"{day}T13:00:00Z" for day in departures], pa.string()
            ).cast(pa.timestamp("ms", tz="UTC")),
            "geometry": pa.array([shapely.to_wkb(line)] * rows, pa.binary()),
            "geometry_projected": pa.array(
                [shapely.to_wkb(projected)] * rows, pa.binary()
            ),
            "country": pa.array(countries, pa.string()),
            "departure_date": pa.array(departures, pa.string()).cast(pa.date32()),
        }
    )




def water_crossing_table(compact_ids, lat_lon, projected):
    rows = len(compact_ids)
    return pa.table(
        {
            "crossing_id": pa.array([f"w{n}-t{n}" for n in range(rows)], pa.string()),
            "crossing_compact_id": pa.array(compact_ids, pa.uint32()),
            "water_id": pa.array(["water-1"] * rows, pa.string()),
            "water_subtype": pa.array(["river"] * rows, pa.string()),
            "water_class": pa.array(["river"] * rows, pa.string()),
            "track_id": pa.array(["track-1"] * rows, pa.string()),
            "rail_id": pa.array(["rail-1"] * rows, pa.string()),
            "rail_class": pa.array(["rail"] * rows, pa.string()),
            "overlap_kind": pa.array(["line"] * rows, pa.string()),
            "overlap_m": pa.array([42.0] * rows, pa.float64()),
            "total_overlap_m": pa.array([58.0] * rows, pa.float64()),
            "merged_parts": pa.array([2] * rows, pa.uint32()),
            "frac": pa.array([0.5] * rows, pa.float64()),
            "extract_id": pa.array(["20260727T193628Z"] * rows, pa.string()),
            "merge_distance_m": pa.array([25.0] * rows, pa.float64()),
            "min_crossing_m": pa.array([5.0] * rows, pa.float64()),
            "geometry": pa.array(
                [shapely.to_wkb(shapely.Point(point)) for point in lat_lon], pa.binary()
            ),
            "geometry_projected": pa.array(
                [shapely.to_wkb(shapely.Point(point)) for point in projected], pa.binary()
            ),
            "country": pa.array(["DE"] * rows, pa.string()),
        }
    )
