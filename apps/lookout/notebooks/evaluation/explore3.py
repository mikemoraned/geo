# /// script
# dependencies = [
#     "marimo",
#     "lookout-evaluation",
#     "lookout-medallion",
#     "lonboard==0.17.0",
#     "geopandas==1.2.0",
#     "matplotlib==3.11.2",
#     "pandas==3.1.1",
#     "shapely==2.2.0",
#     "pyarrow==25.0.0",
# ]
# requires-python = ">=3.13"
#
# [tool.uv.sources]
# lookout-evaluation = { path = "../../crates/evaluation-py" }
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def _():
    import geopandas as gpd
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import pyarrow as pa

    import lookout_evaluation as evaluation
    import lookout_medallion

    return evaluation, gpd, lonboard, lookout_medallion, mo, pa, plt


@app.cell
def _(evaluation, pa):
    picked = pa.table(
        evaluation.pick(
            country="DE", realtime_at_least=90.0, sessions=5, seed=1
        )
    )
    picked.select(
        [
            "session_id",
            "started_at",
            "ended_at",
            "sample_count",
            "realtime_percentage",
        ]
    )
    return (picked,)


@app.cell
def _(evaluation):
    oracle = evaluation.Oracle(horizon="5m")
    scoring = evaluation.Scoring(
        horizon="5m",
        last_moment="30s",
        ramp="1m",
        last_moment_score=0.25,
        floor=0.05,
        warm_up="1m",
    )
    bootstrap = evaluation.Bootstrap(resamples=1000, seed=1)
    return bootstrap, oracle, scoring


@app.cell
def _(evaluation, oracle, pa, picked):
    transitions = pa.table(
        evaluation.replay(country="DE", sessions=picked, predictor=oracle)
    )
    transitions.group_by(["from", "to"]).aggregate([("session_id", "count")])
    return (transitions,)


@app.cell
def _(evaluation, pa, picked, scoring, transitions):
    outcomes = pa.table(
        evaluation.score(
            country="DE",
            sessions=picked,
            transitions=transitions,
            scoring=scoring,
        )
    )
    outcomes.group_by(["outcome", "class"]).aggregate(
        [("session_id", "count"), ("score", "sum")]
    )
    return (outcomes,)


@app.cell
def _(bootstrap, evaluation, outcomes, pa, picked):
    summary = pa.table(
        evaluation.summarise(
            sessions=picked, outcomes=outcomes, bootstrap=bootstrap
        )
    )
    summary
    return


@app.cell
def _(gpd, lookout_medallion, pa, picked):
    _ids = ", ".join(
        f"'{_id}'" for _id in picked.column("session_id").to_pylist()
    )
    paths = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                f"SELECT session_id, geometry FROM train_session_v3 WHERE session_id IN ({_ids})",
                country="DE",
            )
        )
    )
    paths.assign(points=paths.geometry.count_coordinates())[
        ["session_id", "points"]
    ]
    return (paths,)


@app.cell
def _(gpd, lookout_medallion, mo, outcomes, pa):
    _compact_ids = ", ".join(
        str(_id)
        for _id in sorted(
            set(outcomes.column("crossing_compact_id").to_pylist())
        )
    )
    _crossings = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT crossing_compact_id, crossing_id, geometry FROM water_crossing_v2 "
                f"WHERE crossing_compact_id IN ({_compact_ids})",
                country="DE",
            )
        )
    )
    crossed = _crossings.merge(
        outcomes.to_pandas(), on="crossing_compact_id", validate="one_to_many"
    )
    mo.vstack(
        [
            mo.md(
                f"{len(crossed):,} of {outcomes.num_rows:,} outcomes found their crossing"
            ),
            crossed.groupby(["outcome", "class"])
            .size()
            .rename("count")
            .reset_index(),
        ]
    )
    return (crossed,)


@app.cell
def _(crossed, lonboard, mo, paths):
    outcome_colours = {
        "credited": [46, 160, 67],
        "repeat": [130, 80, 223],
        "false_alarm": [235, 104, 52],
        "unscored": [137, 135, 129],
        "miss": [207, 34, 46],
    }

    def outcome_map(session_paths, session_crossed):
        return lonboard.Map(
            [
                lonboard.PathLayer.from_geopandas(
                    session_paths, get_color=[42, 120, 214], width_min_pixels=2
                ),
                lonboard.ScatterplotLayer.from_geopandas(
                    session_crossed[
                        [
                            "session_id",
                            "crossing_id",
                            "outcome",
                            "class",
                            "lead_ms",
                            "score",
                            "geometry",
                        ]
                    ],
                    get_fill_color=lonboard.colormap.apply_categorical_cmap(
                        session_crossed.outcome.astype(str), outcome_colours
                    ),
                    radius_min_pixels=5,
                ),
            ],
            basemap=lonboard.basemap.MaplibreBasemap(
                style=lonboard.basemap.CartoStyle.Positron
            ),
        )

    mo.vstack(
        [
            mo.md(
                f"The {len(paths)} picked sessions in blue, and the crossing of each of their "
                f"{len(crossed):,} outcomes: credited in green, repeat in purple, false alarm in "
                "orange, unscored in grey, and miss in red"
            ),
            outcome_map(paths, crossed),
        ]
    )
    return (outcome_map,)


@app.cell
def _(mo, outcomes, picked):
    _sessions = picked.select(
        ["session_id", "started_at", "ended_at"]
    ).to_pandas()
    _counts = (
        outcomes.to_pandas()
        .groupby(["session_id", "outcome"])
        .size()
        .unstack(fill_value=0)
        .reset_index()
    )
    session_table = mo.ui.table(
        _sessions.assign(
            hours=(
                _sessions.ended_at - _sessions.started_at
            ).dt.total_seconds()
            / 3_600
        )
        .drop(columns=["started_at", "ended_at"])
        .merge(_counts, on="session_id", how="left", validate="one_to_one")
        .fillna(0),
        selection="single",
        label="Select a session to draw it below.",
    )
    session_table
    return (session_table,)


@app.cell
def _(crossed, mo, outcome_map, paths, session_table):
    _selected = session_table.value
    if len(_selected) == 0:
        _shown = mo.md("Select a session in the table above.")
    else:
        _session_id = _selected.session_id.iloc[0]
        _shown = mo.vstack(
            [
                mo.md(f"Session `{_session_id}`"),
                outcome_map(
                    paths[paths.session_id == _session_id],
                    crossed[crossed.session_id == _session_id],
                ),
            ]
        )
    _shown
    return


@app.cell
def _(crossed, mo, picked, plt, session_table, transitions):
    _state_colours = {
        "candidate_pass": "#8a8781",
        "alerted_pass": "#2a78d6",
        "actual_pass": "#2ea043",
    }
    _selected = session_table.value
    if len(_selected) == 0:
        _shown = mo.md("Select a session in the table above.")
    else:
        _session_id = _selected.session_id.iloc[0]
        _ended = (
            picked.to_pandas().set_index("session_id").ended_at[_session_id]
        )
        _all_steps = transitions.to_pandas()
        _steps = _all_steps[_all_steps.session_id == _session_id].sort_values(
            ["crossing_compact_id", "at"]
        )
        _steps = _steps.assign(
            until=_steps.groupby("crossing_compact_id")["at"]
            .shift(-1)
            .fillna(_ended)
        )
        _rows = {
            _crossing: _row
            for _row, _crossing in enumerate(
                _steps.groupby("crossing_compact_id")["at"]
                .min()
                .sort_values()
                .index
            )
        }
        _passes = crossed[crossed.session_id == _session_id].dropna(
            subset=["crossed_at"]
        )
        _figure, _axes = plt.subplots(
            figsize=(10, max(2.5, len(_rows) * 0.06))
        )
        for _state, _colour in _state_colours.items():
            _stretches = _steps[_steps.to == _state]
            _axes.hlines(
                _stretches.crossing_compact_id.map(_rows),
                _stretches["at"],
                _stretches.until,
                colors=_colour,
                linewidth=3,
                label=_state.replace("_", " "),
            )
        _axes.scatter(
            _passes.crossed_at,
            _passes.crossing_compact_id.map(_rows),
            marker="|",
            color="black",
            s=40,
            label="pass",
        )
        _axes.set_yticks([])
        _axes.set_ylabel("crossings, by when first predicted")
        _axes.invert_yaxis()
        _axes.legend(loc="lower left")
        _shown = mo.vstack(
            [mo.md(f"Session `{_session_id}` over time"), _figure]
        )
    _shown
    return


if __name__ == "__main__":
    app.run()
