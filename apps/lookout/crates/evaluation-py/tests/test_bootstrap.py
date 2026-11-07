import datetime

import numpy as np
import pyarrow as pa
import pyarrow.compute as pc
import pytest
from scipy import stats

import lookout_evaluation
from conftest import train_session_table

SESSIONS = [f"session-{n:04}" for n in range(300)]
RESAMPLES = 10_000
NOON = datetime.datetime(2026, 10, 18, 12, tzinfo=datetime.UTC)
OUTCOME = pa.schema(
    [
        ("session_id", pa.string()),
        ("crossing_compact_id", pa.uint32()),
        ("outcome", pa.string()),
        ("notified_at", pa.timestamp("ms", tz="UTC")),
        ("crossed_at", pa.timestamp("ms", tz="UTC")),
        ("lead_ms", pa.int64()),
        ("class", pa.string()),
        ("score", pa.float64()),
    ]
)


@pytest.fixture(scope="module")
def outcomes():
    generator = np.random.default_rng(7)
    rows = []
    for session_id in SESSIONS:
        quality = generator.uniform(0.2, 1.0)
        for crossing in range(generator.integers(1, 30)):
            kind = generator.choice(
                ["credited", "false_alarm", "miss"], p=[quality * 0.8, (1 - quality) * 0.8, 0.2]
            )
            rows.append(
                {
                    "session_id": session_id,
                    "crossing_compact_id": crossing,
                    "outcome": kind,
                    "notified_at": None if kind == "miss" else NOON,
                    "crossed_at": None if kind == "false_alarm" else NOON,
                    "lead_ms": 120_000 if kind == "credited" else None,
                    "class": "hit" if kind == "credited" else None,
                    "score": generator.choice([1.0, 0.525, 0.25, 0.05])
                    if kind == "credited"
                    else 0.0,
                }
            )
    return pa.Table.from_pylist(rows, schema=OUTCOME)


@pytest.fixture(scope="module")
def tallies(outcomes):
    outcome = outcomes.column("outcome")
    tallies = (
        outcomes.append_column(
            "credited", pc.if_else(pc.equal(outcome, "credited"), outcomes.column("score"), 0.0)
        )
        .append_column("notified", pc.cast(pc.not_equal(outcome, "miss"), pa.float64()))
        .append_column("passed", pc.cast(pc.not_equal(outcome, "false_alarm"), pa.float64()))
        .append_column("false_alarmed", pc.cast(pc.equal(outcome, "false_alarm"), pa.float64()))
        .group_by("session_id")
        .aggregate([(column, "sum") for column in ["credited", "notified", "passed", "false_alarmed"]])
        .sort_by("session_id")
    )
    summed = {
        name: tallies.column(f"{name}_sum").to_numpy()
        for name in ["credited", "notified", "passed", "false_alarmed"]
    }
    return summed | {"hours": np.ones(len(SESSIONS))}


@pytest.fixture(scope="module")
def summary(outcomes):
    return pa.table(
        lookout_evaluation.summarise(
            sessions=train_session_table(SESSIONS, [100.0] * len(SESSIONS), lasting=3_600),
            outcomes=outcomes,
            bootstrap=lookout_evaluation.Bootstrap(resamples=RESAMPLES, seed=1),
        )
    ).to_pylist()[0]


def ratio(numerator, denominator, axis=-1):
    return np.sum(numerator, axis=axis) / np.sum(denominator, axis=axis)


def scipy_interval(numerator, denominator):
    interval = stats.bootstrap(
        (numerator, denominator),
        ratio,
        paired=True,
        vectorized=True,
        n_resamples=RESAMPLES,
        method="percentile",
        rng=np.random.default_rng(1),
    ).confidence_interval
    return interval.low, interval.high


@pytest.mark.parametrize(
    "measure, numerator, denominator",
    [
        ("precision", "credited", "notified"),
        ("recall", "credited", "passed"),
        ("false_alarms_per_hour", "false_alarmed", "hours"),
    ],
)
def test_summarise_draws_the_intervals_scipy_draws(
    summary, tallies, measure, numerator, denominator
):
    low, high = scipy_interval(tallies[numerator], tallies[denominator])

    tolerance = 0.05 * (high - low)
    assert summary[measure] == pytest.approx(ratio(tallies[numerator], tallies[denominator]))
    assert summary[f"{measure}_low"] == pytest.approx(low, abs=tolerance)
    assert summary[f"{measure}_high"] == pytest.approx(high, abs=tolerance)
