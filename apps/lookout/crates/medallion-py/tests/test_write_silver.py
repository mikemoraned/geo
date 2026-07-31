"""What a notebook gets when it writes a silver dataset.

The checks that matter here are the ones a Rust test cannot make: that a table built by the
engines a notebook actually uses — pyarrow, DuckDB — crosses into the store intact, and that
what comes back out is readable by an engine that was not involved in writing it.
"""

import datetime

import duckdb
import pyarrow as pa
import pytest
import shapely

import lookout_medallion

# Berlin and Frankfurt in lat/lon, and the same two points in the zone Germany's projected
# geometry uses (EPSG:25832), so the table carries both columns the dataset holds without
# needing a projection library here.
BERLIN = (13.404954, 52.520008)
FRANKFURT = (8.682127, 50.110924)
BERLIN_UTM32N = (798809.63, 5828000.60)
PROJECTED_CRS = "EPSG:25832"
FRANKFURT_UTM32N = (477271.45, 5551012.24)


@pytest.fixture
def store(tmp_path):
    return tmp_path


def leg_table(trip_ids, departures, countries):
    """A table shaped like the silver `train_segment` dataset."""
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


def read(store, sql):
    """Read the store back with DuckDB — an engine that had no part in writing it.

    Plain DuckDB, without its spatial extension: a geometry column is WKB, so the geometry
    can be decoded here, and the read then depends on nothing but parquet.
    """
    return duckdb.connect().sql(sql.format(store=store)).fetchall()


class TestWritingATable:
    def test_a_table_lands_in_one_file_per_partition(self, store):
        table = leg_table(["a", "b"], ["2026-07-21", "2026-07-22"], ["DE", "DE"])

        written = lookout_medallion.write_silver(
            "train_segment", table, root=str(store)
        )

        assert written.rows == 2
        assert written.partitions_written == 2
        assert written.partitions_removed == 0
        assert (
            store
            / "silver/train_segment/country=DE/departure_date=2026-07-21/part-0.parquet"
        ).exists()

    def test_the_rows_read_back_through_another_engine(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"])
        lookout_medallion.write_silver("train_segment", table, root=str(store))

        rows = read(
            store,
            "SELECT trip_id, departure, geometry, geometry_projected "
            "FROM read_parquet('{store}/silver/train_segment/**/*.parquet')",
        )

        trip_id, departure, geometry, projected = rows[0]
        assert trip_id == "a"
        assert departure == datetime.datetime(
            2026, 7, 21, 9, tzinfo=datetime.timezone.utc
        )
        assert shapely.from_wkb(bytes(geometry)).coords[0] == BERLIN
        assert shapely.from_wkb(bytes(projected)).coords[0] == BERLIN_UTM32N

    def test_the_partition_values_come_back_as_columns(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"])
        lookout_medallion.write_silver("train_segment", table, root=str(store))

        rows = read(
            store,
            "SELECT country, departure_date FROM read_parquet("
            "'{store}/silver/train_segment/**/*.parquet', hive_partitioning = true)",
        )

        assert rows == [("DE", datetime.date(2026, 7, 21))]

    def test_a_duckdb_result_can_be_handed_over_directly(self, store):
        """The engine a notebook queries with, passed with no copy through pyarrow."""
        table = leg_table(["a"], ["2026-07-21"], ["DE"])
        con = duckdb.connect()
        con.register("legs", table)

        written = lookout_medallion.write_silver(
            "train_segment", con.sql("SELECT * FROM legs"), root=str(store)
        )

        assert written.rows == 1


class TestRewriting:
    def test_a_partition_the_table_no_longer_covers_is_removed(self, store):
        lookout_medallion.write_silver(
            "train_segment",
            leg_table(["a", "b"], ["2026-07-21", "2026-07-22"], ["DE", "DE"]),
            root=str(store),
        )

        written = lookout_medallion.write_silver(
            "train_segment",
            leg_table(["a"], ["2026-07-21"], ["DE"]),
            root=str(store),
        )

        assert written.partitions_removed == 1
        assert not (
            store / "silver/train_segment/country=DE/departure_date=2026-07-22"
        ).exists()

    def test_rewriting_the_same_table_leaves_the_same_rows(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"])
        lookout_medallion.write_silver("train_segment", table, root=str(store))
        lookout_medallion.write_silver("train_segment", table, root=str(store))

        rows = read(
            store,
            "SELECT count(*) FROM read_parquet("
            "'{store}/silver/train_segment/**/*.parquet')",
        )

        assert rows == [(1,)]


class TestTheCrossingDatasets:
    """The two layouts the sessions and legs do not exercise: a dataset partitioned by
    country alone, and a dated one carrying no geometry at all."""

    def test_a_water_crossing_lands_under_its_country(self, store):
        point = shapely.Point(BERLIN)
        table = pa.table(
            {
                "crossing_id": pa.array(["w1-t1"], pa.string()),
                "water_id": pa.array(["water-1"], pa.string()),
                "water_subtype": pa.array(["river"], pa.string()),
                "water_class": pa.array(["river"], pa.string()),
                "track_id": pa.array(["track-1"], pa.string()),
                "rail_id": pa.array(["rail-1"], pa.string()),
                "rail_class": pa.array(["rail"], pa.string()),
                "overlap_kind": pa.array(["line"], pa.string()),
                "overlap_m": pa.array([42.0], pa.float64()),
                "total_overlap_m": pa.array([58.0], pa.float64()),
                "merged_parts": pa.array([2], pa.uint32()),
                "frac": pa.array([0.5], pa.float64()),
                "extract_id": pa.array(["20260727T193628Z"], pa.string()),
                "merge_distance_m": pa.array([25.0], pa.float64()),
                "min_crossing_m": pa.array([5.0], pa.float64()),
                "geometry": pa.array([shapely.to_wkb(point)], pa.binary()),
                "geometry_projected": pa.array(
                    [shapely.to_wkb(shapely.Point(BERLIN_UTM32N))], pa.binary()
                ),
                "country": pa.array(["DE"], pa.string()),
            }
        )

        written = lookout_medallion.write_silver(
            "water_crossing", table, root=str(store)
        )

        assert written.rows == 1
        assert (store / "silver/water_crossing/country=DE/part-0.parquet").exists()
        rows = read(
            store,
            "SELECT overlap_kind, geometry FROM read_parquet("
            "'{store}/silver/water_crossing/**/*.parquet')",
        )
        assert rows[0][0] == "line"
        assert shapely.from_wkb(bytes(rows[0][1])).coords[0] == BERLIN

    def test_a_session_crossing_is_dated_and_holds_no_geometry(self, store):
        table = pa.table(
            {
                "session_id": pa.array(["s1"], pa.string()),
                "crossing_id": pa.array(["w1-t1"], pa.string()),
                "device_id": pa.array(["device-a"], pa.string()),
                "crossed_at": pa.array(["2026-07-21T09:14:00Z"], pa.string()).cast(
                    pa.timestamp("ms", tz="UTC")
                ),
                "distance_m": pa.array([12.5], pa.float64()),
                "samples_within": pa.array([4], pa.uint32()),
                "match_radius_m": pa.array([50.0], pa.float64()),
                "crossed_date": pa.array(["2026-07-21"], pa.string()).cast(pa.date32()),
            }
        )

        written = lookout_medallion.write_silver(
            "session_crossing", table, root=str(store)
        )

        assert written.rows == 1
        assert (
            store / "silver/session_crossing/crossed_date=2026-07-21/part-0.parquet"
        ).exists()


class TestWhatIsRefused:
    def test_a_dataset_the_store_does_not_define(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"])

        with pytest.raises(ValueError, match="crossing_candidates"):
            lookout_medallion.write_silver(
                "crossing_candidates", table, root=str(store)
            )

    def test_a_bronze_dataset(self, store):
        """Bronze is what cannot be re-derived, and a table write replaces."""
        table = leg_table(["a"], ["2026-07-21"], ["DE"])

        with pytest.raises(ValueError, match="gps_reading"):
            lookout_medallion.write_silver("gps_reading", table, root=str(store))

    def test_a_column_the_dataset_holds_but_the_table_does_not(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"]).drop_columns(["mode"])

        with pytest.raises(ValueError, match="mode"):
            lookout_medallion.write_silver("train_segment", table, root=str(store))

    def test_a_column_the_dataset_does_not_hold(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"]).append_column(
            "scratch", pa.array(["x"], pa.string())
        )

        with pytest.raises(ValueError, match="scratch"):
            lookout_medallion.write_silver("train_segment", table, root=str(store))

    def test_a_country_the_store_does_not_know(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["ZZ"])

        with pytest.raises(ValueError, match="ZZ"):
            lookout_medallion.write_silver("train_segment", table, root=str(store))

    def test_nothing_is_written_when_the_table_is_refused(self, store):
        table = leg_table(["a"], ["2026-07-21"], ["DE"]).drop_columns(["mode"])

        with pytest.raises(ValueError):
            lookout_medallion.write_silver("train_segment", table, root=str(store))

        assert not (store / "silver").exists()


class TestTheProjectedCrs:
    def test_a_country_names_the_zone_the_store_projects_it_into(self):
        assert lookout_medallion.projected_crs("DE") == PROJECTED_CRS

    def test_the_code_is_read_in_either_case(self):
        assert lookout_medallion.projected_crs("de") == PROJECTED_CRS

    def test_a_country_the_store_does_not_know(self):
        with pytest.raises(ValueError, match="ZZ"):
            lookout_medallion.projected_crs("ZZ")
