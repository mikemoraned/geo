import pyarrow as pa
import pytest

import lookout_evaluation
import lookout_medallion
from conftest import crossing_table, train_pass_table, train_sample_table, train_session_table

SESSION_IDS = ["session-a", "session-b", "session-c"]
CROSSINGS = [("crossing-1", 1), ("crossing-2", 2)]


@pytest.fixture
def written(store):
    root = str(store)
    lookout_medallion.write_silver(
        "train_session_v3",
        train_session_table(SESSION_IDS, [100.0] * len(SESSION_IDS), lasting=600),
        root=root,
    )
    lookout_medallion.write_silver(
        "train_session_sample_v3",
        train_sample_table(SESSION_IDS, [step * 10 for step in range(61)]),
        root=root,
    )
    lookout_medallion.write_silver(
        "train_session_crossing_v1",
        train_pass_table([(session_id, "crossing-1", 300) for session_id in SESSION_IDS]),
        root=root,
    )
    lookout_medallion.write_silver("water_crossing_v2", crossing_table(CROSSINGS), root=root)
    return root


def first_runs():
    return lookout_evaluation.Scoring(
        horizon="5m",
        last_moment="30s",
        ramp="1m",
        last_moment_score=0.25,
        floor=0.05,
        warm_up="1m",
    )


def bootstrap():
    return lookout_evaluation.Bootstrap(resamples=100, seed=1)


def oracle():
    return lookout_evaluation.Oracle(horizon="5m")


def run(root, predictor):
    ran = lookout_evaluation.run(
        country="DE",
        realtime_at_least=90.0,
        sessions=len(SESSION_IDS),
        seed=1,
        predictor=predictor,
        scoring=first_runs(),
        bootstrap=bootstrap(),
        root=root,
    )
    return {name: pa.table(table) for name, table in ran.items()}


def test_the_oracle_run_scores_perfectly(written):
    summary = run(written, oracle())["summary"].to_pylist()[0]

    assert (summary["passes"], summary["notifications"], summary["misses"]) == (3, 3, 0)
    assert (summary["precision"], summary["recall"]) == (1.0, 1.0)
    assert (summary["precision_low"], summary["recall_low"]) == (1.0, 1.0)


def test_the_steps_run_one_by_one_give_what_the_run_gives(written):
    ran = run(written, oracle())
    picked = ran["picked"]

    transitions = pa.table(
        lookout_evaluation.replay(country="DE", sessions=picked, predictor=oracle(), root=written)
    )
    outcomes = pa.table(
        lookout_evaluation.score(
            country="DE",
            sessions=picked,
            transitions=transitions,
            scoring=first_runs(),
            root=written,
        )
    )
    summary = pa.table(
        lookout_evaluation.summarise(sessions=picked, outcomes=outcomes, bootstrap=bootstrap())
    )

    assert transitions.equals(ran["transitions"])
    assert outcomes.equals(ran["outcomes"])
    assert summary.equals(ran["summary"])


def test_a_transition_is_a_row_of_session_crossing_instant_and_states(written):
    transitions = run(written, oracle())["transitions"]

    assert transitions.column_names == ["session_id", "crossing_compact_id", "at", "from", "to"]
    assert set(transitions.column("to").to_pylist()) == {
        "alerted_pass",
        "actual_pass",
        "out_of_reach",
    }


def test_crow_flies_notifies_nothing_and_misses_every_pass(written):
    summary = run(written, lookout_evaluation.CrowFlies(radius_metres=5_000.0, horizon="5m"))["summary"]

    assert summary.to_pylist()[0]["notifications"] == 0
    assert summary.to_pylist()[0]["misses"] == 3


def test_an_unknown_float_type_is_refused(written):
    picked = run(written, oracle())["picked"]

    with pytest.raises(ValueError, match="float_type"):
        lookout_evaluation.replay(
            country="DE", sessions=picked, predictor=oracle(), float_type="f16", root=written
        )


def test_a_predictor_that_is_neither_an_oracle_nor_crow_flies_is_refused(written):
    picked = run(written, oracle())["picked"]

    with pytest.raises(ValueError, match="predictor"):
        lookout_evaluation.replay(country="DE", sessions=picked, predictor="oracle", root=written)


def test_transitions_of_a_session_not_picked_are_refused(written):
    ran = run(written, oracle())
    fewer = ran["picked"].slice(0, 1)

    with pytest.raises(ValueError, match="session-b"):
        lookout_evaluation.score(
            country="DE",
            sessions=fewer,
            transitions=ran["transitions"],
            scoring=first_runs(),
            root=written,
        )


def recording(written, picked):
    return dict(
        country="DE",
        realtime_at_least=90.0,
        sessions=len(SESSION_IDS),
        seed=1,
        predictor=oracle(),
        scoring=first_runs(),
        bootstrap=bootstrap(),
        picked=picked,
        root=written,
    )


def test_a_record_names_the_run_its_settings_code_and_data_make(written):
    picked = run(written, oracle())["picked"]
    code = lookout_evaluation.BUILD_CODE_VERSION
    if code == "unknown" or code.endswith("-dirty"):
        with pytest.raises(ValueError, match="code"):
            lookout_evaluation.record(**recording(written, picked))
        return

    recorded = lookout_evaluation.record(**recording(written, picked))
    again = lookout_evaluation.record(**recording(written, picked))

    run_row = pa.table(recorded["run"]).to_pylist()[0]
    assert recorded["run_id"] == again["run_id"]
    assert run_row["code_version"] == code
    assert recorded["run_date"] == run_row["started_at"].date().isoformat()
    assert pa.table(recorded["sessions"]).column("session_id").to_pylist() == SESSION_IDS
    written_run = lookout_medallion.write_gold(
        "evaluation_run_v3",
        recorded["run"],
        run_date=recorded["run_date"],
        run_id=recorded["run_id"],
        root=written,
    )
    assert written_run.rows == 1


def evaluating(written):
    return dict(
        country="DE",
        realtime_at_least=90.0,
        sessions=len(SESSION_IDS),
        seed=1,
        predictor=oracle(),
        scoring=first_runs(),
        bootstrap=bootstrap(),
        root=written,
    )


def test_an_evaluation_writes_to_gold_the_tables_it_returns(written):
    code = lookout_evaluation.BUILD_CODE_VERSION
    if code == "unknown" or code.endswith("-dirty"):
        with pytest.raises(ValueError, match="code"):
            lookout_evaluation.evaluate(**evaluating(written))
        return

    evaluated = lookout_evaluation.evaluate(**evaluating(written))

    for dataset, name in [
        ("evaluation_transition_v1", "transitions"),
        ("evaluation_outcome_v1", "outcomes"),
        ("evaluation_summary_v1", "summary"),
    ]:
        stored = pa.table(
            lookout_medallion.query_gold(
                f"SELECT * FROM {dataset} WHERE run_id = '{evaluated['run_id']}'",
                root=written,
            )
        )
        assert stored.num_rows == pa.table(evaluated[name]).num_rows
    with pytest.raises(ValueError, match="already"):
        lookout_evaluation.evaluate(**evaluating(written))
