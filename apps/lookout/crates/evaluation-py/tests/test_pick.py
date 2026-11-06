import hashlib

import pyarrow as pa
import pytest

import lookout_evaluation
import lookout_medallion
from conftest import train_session_table

SESSION_IDS = [f"session-{n:05}" for n in range(100)]


@pytest.fixture
def written(store):
    lookout_medallion.write_silver(
        "train_session_v3",
        train_session_table(SESSION_IDS, [100.0 if n % 2 == 0 else 50.0 for n in range(100)]),
        root=str(store),
    )
    return store


def pick(written, **kwargs):
    arguments = dict(country="DE", realtime_at_least=90.0, sessions=10, seed=1, root=str(written))
    return pa.table(lookout_evaluation.pick(**(arguments | kwargs)))


def ids(table):
    return table.column("session_id").to_pylist()


def test_a_draw_holds_as_many_sessions_as_asked_for_in_order_of_id(written):
    picked = ids(pick(written))

    assert len(picked) == 10
    assert picked == sorted(set(picked))


def test_a_draw_holds_only_sessions_at_or_above_the_realtime_percentage(written):
    picked = pick(written, sessions=50)

    assert set(picked.column("realtime_percentage").to_pylist()) == {100.0}


def test_a_seed_draws_the_sessions_whose_salted_hashes_come_first(written):
    eligible = SESSION_IDS[::2]

    def salted(session_id):
        return hashlib.sha256(f"7:{session_id}".encode()).digest(), session_id

    assert ids(pick(written, seed=7)) == sorted(sorted(eligible, key=salted)[:10])


def test_a_draw_carries_no_geometry(written):
    names = pick(written).column_names

    assert "geometry" not in names
    assert "geometry_projected" not in names


def test_a_draw_larger_than_the_sessions_passing_the_filter_raises_a_value_error(written):
    with pytest.raises(ValueError, match="only 50 pass the filter"):
        pick(written, sessions=51)


def test_a_country_holding_no_train_sessions_raises_a_value_error(written):
    with pytest.raises(ValueError, match="does not exist"):
        pick(written, country="GB")


@pytest.mark.parametrize(
    "argument",
    [{"country": "XX"}, {"realtime_at_least": 100.5}, {"sessions": 0}],
)
def test_an_argument_out_of_range_raises_a_value_error(written, argument):
    with pytest.raises(ValueError):
        pick(written, **argument)
