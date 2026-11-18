import pyarrow as pa
import pyarrow.parquet as pq
import pytest

import lookout_medallion


def land(store, polled_date, file, trip_ids):
    path = store / "landing" / "motis_poll_v2" / f"polled_date={polled_date}" / file
    path.parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(pa.table({"trip_id": pa.array(trip_ids, pa.string())}), path)


@pytest.fixture
def landed(store):
    land(store, "2026-11-18", "20261118T202221900Z.parquet", ["sr-1", "sr-2"])
    land(store, "2026-11-18", "20261118T203221900Z.parquet", ["sr-3"])
    land(store, "2026-11-19", "20261119T000221900Z.parquet", ["sr-4"])
    return store


def query(landed, sql, **kwargs):
    return pa.table(lookout_medallion.query_landing(sql, root=str(landed), **kwargs))


def test_every_poll_landed_reads_as_one_table(landed):
    table = query(landed, "SELECT trip_id FROM motis_poll_v2 ORDER BY trip_id")

    assert table.column("trip_id").to_pylist() == ["sr-1", "sr-2", "sr-3", "sr-4"]


def test_the_poll_date_comes_back_as_a_column(landed):
    table = query(
        landed,
        "SELECT trip_id FROM motis_poll_v2 WHERE polled_date = $day ORDER BY trip_id",
        params={"day": "2026-11-19"},
    )

    assert table.column("trip_id").to_pylist() == ["sr-4"]


def test_a_dataset_nothing_has_landed_in_is_reported(store):
    with pytest.raises(ValueError, match="motis_poll_v2"):
        query(store, "SELECT * FROM motis_poll_v2")


def test_a_dataset_landing_does_not_hold_is_refused_with_the_known_ones(landed):
    with pytest.raises(ValueError, match="motis_poll_v2"):
        query(landed, "SELECT * FROM motis_segment_v3")
