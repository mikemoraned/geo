"""What a reader gets when it queries the store.

The store answers with an Arrow table, so what these check is the crossing: that the
datasets a query reads are the tables it names, that a value binds as a value, that geometry
comes back as geometry rather than as bytes to decode, and that a mistake is raised as one.
"""

import datetime

import pyarrow as pa
import pytest

import lookout_medallion
from conftest import BERLIN, BERLIN_UTM32N, crossing_table, leg_table


@pytest.fixture
def written(store):
    """A store holding two legs and one crossing."""
    lookout_medallion.write_silver(
        "train_segment",
        leg_table(["a", "b"], ["2026-07-21", "2026-07-22"], ["DE", "DE"]),
        root=str(store),
    )
    lookout_medallion.write_silver(
        "water_crossing",
        crossing_table([0x292E417A], [BERLIN], [BERLIN_UTM32N]),
        root=str(store),
    )
    return store


def query(written, sql, **kwargs):
    return pa.table(lookout_medallion.query_silver(sql, root=str(written), **kwargs))


def test_a_dataset_is_read_by_name_across_every_partition_it_holds(written):
    table = query(
        written,
        "SELECT trip_id, departure FROM train_segment ORDER BY trip_id",
    )

    assert table.column("trip_id").to_pylist() == ["a", "b"]
    assert table.column("departure").to_pylist()[0] == datetime.datetime(
        2026, 7, 21, 9, tzinfo=datetime.UTC
    )


def test_a_partition_value_comes_back_as_a_column(written):
    table = query(
        written,
        "SELECT DISTINCT country FROM train_segment",
    )

    assert table.column("country").to_pylist() == ["DE"]


def test_a_parameter_binds_as_a_value(written):
    table = query(
        written,
        "SELECT trip_id FROM train_segment WHERE trip_id = $trip",
        params={"trip": "b"},
    )

    assert table.column("trip_id").to_pylist() == ["b"]


def test_a_parameter_carrying_a_quote_matches_nothing(written):
    table = query(
        written,
        "SELECT trip_id FROM train_segment WHERE trip_id = $trip",
        params={"trip": "b' OR '1' = '1"},
    )

    assert table.num_rows == 0


def test_geometry_reads_back_as_geometry_rather_than_as_bytes(written):
    """The point of naming the dataset: its geometry column arrives with its CRS, so a
    reader asks for a coordinate instead of decoding WKB itself."""
    table = query(
        written,
        "SELECT ST_X(geometry) AS lon, ST_Y(geometry) AS lat FROM water_crossing",
    )

    assert (table.column("lon")[0].as_py(), table.column("lat")[0].as_py()) == BERLIN


def test_two_datasets_can_be_read_in_one_query(written):
    table = query(
        written,
        "SELECT c.crossing_compact_id, s.trip_id FROM water_crossing c, train_segment s "
        "WHERE s.trip_id = 'a'",
    )

    assert table.column("trip_id").to_pylist() == ["a"]


def test_a_query_matching_nothing_still_names_its_columns(written):
    table = query(
        written,
        "SELECT trip_id FROM train_segment WHERE trip_id = 'none'",
    )

    assert table.num_rows == 0
    assert table.column_names == ["trip_id"]


class TestWhatIsRefused:
    def test_a_dataset_the_store_does_not_define(self, written):
        with pytest.raises(ValueError, match="crossing_candidates"):
            query(written, "SELECT * FROM crossing_candidates")

    def test_a_dataset_that_has_never_been_written(self, store):
        with pytest.raises(ValueError, match="session_sample"):
            query(store, "SELECT * FROM session_sample")

    def test_sql_that_does_not_parse(self, written):
        with pytest.raises(ValueError):
            query(written, "SELECT * FROM (")

    def test_a_parameter_of_a_type_the_store_cannot_bind(self, written):
        with pytest.raises(TypeError):
            query(
                written,
                "SELECT trip_id FROM train_segment WHERE trip_id = $trip",
                params={"trip": {"not": "a value"}},
            )
