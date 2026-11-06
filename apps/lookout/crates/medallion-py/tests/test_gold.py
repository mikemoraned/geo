import datetime

import pyarrow as pa
import pytest

import lookout_medallion

NOON = datetime.datetime(2026, 11, 6, 12, 0, tzinfo=datetime.UTC)


def transition_table(session_ids):
    rows = len(session_ids)
    return pa.table(
        {
            "session_id": pa.array(session_ids, pa.string()),
            "crossing_compact_id": pa.array(range(rows), pa.uint32()),
            "at": pa.array([NOON] * rows, pa.timestamp("ms", tz="UTC")),
            "from": pa.array(["out_of_reach"] * rows, pa.string()),
            "to": pa.array(["alerted_pass"] * rows, pa.string()),
        }
    )


def write(store, session_ids, run_id="abc", run_date="2026-11-06"):
    return lookout_medallion.write_gold(
        "evaluation_transition_v1",
        transition_table(session_ids),
        run_date=run_date,
        run_id=run_id,
        root=str(store),
    )


def query(store, sql):
    return pa.table(lookout_medallion.query_gold(sql, root=str(store)))


def test_a_written_run_is_read_back_with_its_date_and_id(store):
    written = write(store, ["session-a", "session-b"])

    assert (written.rows, written.partitions_written, written.partitions_removed) == (2, 1, 0)
    assert (
        store / "gold/evaluation_transition_v1/run_date=2026-11-06/run_id=abc/part-0.parquet"
    ).exists()
    assert query(
        store,
        "SELECT run_date, run_id, session_id FROM evaluation_transition_v1 ORDER BY session_id",
    ).to_pylist() == [
        {"run_date": "2026-11-06", "run_id": "abc", "session_id": "session-a"},
        {"run_date": "2026-11-06", "run_id": "abc", "session_id": "session-b"},
    ]


def test_a_query_picks_one_run_by_its_id(store):
    write(store, ["session-a"], run_id="abc")
    write(store, ["session-b", "session-c"], run_id="def", run_date="2026-11-07")

    picked = query(store, "SELECT session_id FROM evaluation_transition_v1 WHERE run_id = 'def'")

    assert sorted(picked.column("session_id").to_pylist()) == ["session-b", "session-c"]


def test_a_run_written_again_is_refused(store):
    write(store, ["session-a"])

    with pytest.raises(ValueError, match="already holds run abc"):
        write(store, ["session-b"], run_date="2026-11-07")


def test_a_dataset_gold_does_not_hold_is_refused(store):
    with pytest.raises(ValueError, match="no gold dataset named `train_session_v3`"):
        lookout_medallion.write_gold(
            "train_session_v3",
            transition_table(["session-a"]),
            run_date="2026-11-06",
            run_id="abc",
            root=str(store),
        )


def test_a_run_date_that_is_not_a_date_is_refused(store):
    with pytest.raises(ValueError, match="run_date"):
        write(store, ["session-a"], run_date="6 November")


def test_a_table_lacking_a_column_of_the_dataset_is_refused(store):
    with pytest.raises(ValueError):
        lookout_medallion.write_gold(
            "evaluation_transition_v1",
            transition_table(["session-a"]).drop_columns(["to"]),
            run_date="2026-11-06",
            run_id="abc",
            root=str(store),
        )


def test_a_query_of_a_gold_dataset_never_written_is_refused(store):
    with pytest.raises(ValueError, match="evaluation_summary_v1"):
        query(store, "SELECT * FROM evaluation_summary_v1")
