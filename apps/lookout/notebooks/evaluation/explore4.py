# /// script
# dependencies = [
#     "geopandas==1.2.0",
#     "lonboard==0.17.0",
#     "lookout-evaluation",
#     "lookout-medallion",
#     "marimo",
#     "matplotlib==3.11.2",
#     "pandas==3.1.1",
#     "pyarrow==25.0.0",
#     "shapely==2.2.0",
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
def imports():
    import geopandas as gpd
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import pandas as pd
    import pyarrow as pa

    import lookout_medallion

    return gpd, lonboard, lookout_medallion, mo, pa, plt


@app.cell
def intro(mo):
    mo.md("""
    # Where CrowFlies goes wrong

    Every run of CrowFlies scores far below the oracle. This notebook maps where its
    notifications go wrong, for one run chosen below. A **false alarm** is a notification of a
    crossing with no pass still to come. A pass credited **too early** or at the **last moment**
    earns only part of its score, which is where recall is lost. No CrowFlies run has a miss, so
    there is no map of misses. The last section follows one session's transitions over time.
    """)
    return


@app.cell
def runs(lookout_medallion, pa):
    runs = pa.table(
        lookout_medallion.query_gold(
            "SELECT run.run_id, run.sessions, run.predictor_radius_m, summary.precision, "
            "summary.recall, summary.false_alarms_per_hour "
            "FROM evaluation_run_v2 AS run JOIN evaluation_summary_v1 AS summary USING (run_id) "
            "WHERE run.predictor = 'crow_flies' "
            "ORDER BY run.sessions DESC, run.predictor_radius_m"
        )
    ).to_pandas()
    runs
    return (runs,)


@app.cell
def run_choice(mo, runs):
    run_choice = mo.ui.dropdown(
        options={
            f"{_run.predictor_radius_m / 1000:g} km over {_run.sessions:,} sessions": _run.run_id
            for _run in runs.itertuples()
        },
        value="1 km over 34,521 sessions",
        label="CrowFlies run",
    )
    run_choice
    return (run_choice,)


@app.cell
def run_id(mo, run_choice):
    run_id = run_choice.value
    mo.md(f"Run `{run_id}`")
    return (run_id,)


@app.cell
def outcome_counts(lookout_medallion, pa, run_id):
    outcome_counts = pa.table(
        lookout_medallion.query_gold(
            "SELECT outcome, class, COUNT(*) AS notifications FROM evaluation_outcome_v1 "
            "WHERE run_id = $run_id GROUP BY outcome, class ORDER BY notifications DESC",
            params={"run_id": run_id},
        )
    ).to_pandas()
    outcome_counts
    return


@app.cell
def crossings(gpd, lookout_medallion, mo, pa):
    crossings = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT crossing_compact_id, crossing_id, geometry FROM water_crossing_v2",
                country="DE",
            )
        )
    )
    mo.md(f"{len(crossings):,} crossings in DE")
    return (crossings,)


@app.cell
def false_alarms_heading(mo):
    mo.md("""
    ## False alarms by crossing

    Each crossing is sized and coloured by how many false alarms it raised across the run.
    """)
    return


@app.cell
def false_alarm_counts(lookout_medallion, mo, pa, run_id):
    false_alarm_counts = pa.table(
        lookout_medallion.query_gold(
            "SELECT crossing_compact_id, COUNT(*) AS false_alarms, "
            "COUNT(DISTINCT session_id) AS sessions FROM evaluation_outcome_v1 "
            "WHERE run_id = $run_id AND outcome = 'false_alarm' "
            "GROUP BY crossing_compact_id ORDER BY false_alarms DESC",
            params={"run_id": run_id},
        )
    ).to_pandas()
    mo.md(
        f"{false_alarm_counts.false_alarms.sum():,} false alarms on "
        f"{len(false_alarm_counts):,} crossings"
    )
    return (false_alarm_counts,)


@app.cell
def false_alarm_crossings(crossings, false_alarm_counts, mo):
    false_alarm_crossings = crossings.merge(
        false_alarm_counts, on="crossing_compact_id", validate="one_to_one"
    ).sort_values("false_alarms", ascending=False)
    mo.vstack(
        [
            mo.md(
                f"{len(false_alarm_crossings):,} of {len(false_alarm_counts):,} crossings "
                "with false alarms found their place"
            ),
            false_alarm_crossings.drop(columns="geometry").head(10),
        ]
    )
    return (false_alarm_crossings,)


@app.cell
def false_alarm_map(false_alarm_crossings, lonboard, plt):
    _shares = (
        false_alarm_crossings.false_alarms
        / false_alarm_crossings.false_alarms.max()
    )
    lonboard.Map(
        lonboard.ScatterplotLayer.from_geopandas(
            false_alarm_crossings[
                ["crossing_id", "false_alarms", "sessions", "geometry"]
            ],
            get_fill_color=lonboard.colormap.apply_continuous_cmap(
                _shares.to_numpy(), plt.get_cmap("plasma")
            ),
            get_radius=(
                false_alarm_crossings.false_alarms.pow(0.5) * 30
            ).to_numpy(),
            radius_min_pixels=2,
        ),
        basemap=lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        ),
    )
    return


@app.cell
def false_alarm_timing_heading(mo):
    mo.md("""
    A false alarm has no pass of its crossing still to come. So it either follows the
    crossing's last pass in its session, or falls on a crossing the session never passes.
    """)
    return


@app.cell
def false_alarm_timing(lookout_medallion, pa, run_id):
    false_alarm_timing = pa.table(
        lookout_medallion.query_gold(
            "SELECT CASE WHEN passed.last_pass IS NULL THEN 'crossing never passed' "
            "ELSE 'after the last pass of its crossing' END AS timing, COUNT(*) AS false_alarms "
            "FROM evaluation_outcome_v1 AS alarm "
            "LEFT JOIN ("
            "SELECT session_id, crossing_compact_id, MAX(crossed_at) AS last_pass "
            "FROM evaluation_outcome_v1 WHERE run_id = $run_id AND crossed_at IS NOT NULL "
            "GROUP BY session_id, crossing_compact_id"
            ") AS passed "
            "ON alarm.session_id = passed.session_id "
            "AND alarm.crossing_compact_id = passed.crossing_compact_id "
            "WHERE alarm.run_id = $run_id AND alarm.outcome = 'false_alarm' "
            "GROUP BY timing ORDER BY false_alarms DESC",
            params={"run_id": run_id},
        )
    ).to_pandas()
    false_alarm_timing.assign(
        share=false_alarm_timing.false_alarms
        / false_alarm_timing.false_alarms.sum()
    )
    return


@app.cell
def lost_recall_heading(mo):
    mo.md("""
    ## Passes credited too early or at the last moment

    Each crossing whose passes were credited outside the hit window, coloured by class and
    sized by how many such passes it had.
    """)
    return


@app.cell
def lost_recall_counts(lookout_medallion, pa, run_id):
    lost_recall_counts = pa.table(
        lookout_medallion.query_gold(
            "SELECT crossing_compact_id, class, COUNT(*) AS passes FROM evaluation_outcome_v1 "
            "WHERE run_id = $run_id AND outcome = 'credited' AND class <> 'hit' "
            "GROUP BY crossing_compact_id, class ORDER BY passes DESC",
            params={"run_id": run_id},
        )
    ).to_pandas()
    lost_recall_counts.groupby("class").passes.agg(["sum", "count"]).rename(
        columns={"sum": "passes", "count": "crossings"}
    )
    return (lost_recall_counts,)


@app.cell
def lost_recall_crossings(crossings, lost_recall_counts, mo):
    lost_recall_crossings = crossings.merge(
        lost_recall_counts, on="crossing_compact_id", validate="one_to_many"
    ).sort_values("passes", ascending=False)
    mo.vstack(
        [
            mo.md(
                f"{len(lost_recall_crossings):,} of {len(lost_recall_counts):,} rows "
                "found their crossing"
            ),
            lost_recall_crossings.drop(columns="geometry").head(10),
        ]
    )
    return (lost_recall_crossings,)


@app.cell
def lost_recall_map(lonboard, lost_recall_crossings):
    class_colours = {
        "too_early": [42, 120, 214],
        "last_moment": [235, 104, 52],
    }
    lonboard.Map(
        lonboard.ScatterplotLayer.from_geopandas(
            lost_recall_crossings[
                ["crossing_id", "class", "passes", "geometry"]
            ],
            get_fill_color=lonboard.colormap.apply_categorical_cmap(
                lost_recall_crossings["class"].astype(str), class_colours
            ),
            get_radius=(lost_recall_crossings.passes.pow(0.5) * 30).to_numpy(),
            radius_min_pixels=2,
        ),
        basemap=lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        ),
    )
    return


@app.cell
def session_heading(mo):
    mo.md("""
    ## One session over time

    The sessions with the most false alarms, highest first. Select one to map its path and
    draw its crossings' states over time.
    """)
    return


@app.cell
def session_ranking(lookout_medallion, mo, pa, run_id):
    session_ranking = pa.table(
        lookout_medallion.query_gold(
            "SELECT outcome.session_id, session.started_at, session.ended_at, session.hours, "
            "COUNT(*) AS false_alarms, COUNT(*) / session.hours AS false_alarms_per_hour "
            "FROM evaluation_outcome_v1 AS outcome "
            "JOIN evaluation_session_v1 AS session "
            "ON outcome.session_id = session.session_id AND outcome.run_id = session.run_id "
            "WHERE outcome.run_id = $run_id AND outcome.outcome = 'false_alarm' "
            "GROUP BY outcome.session_id, session.started_at, session.ended_at, session.hours "
            "ORDER BY false_alarms DESC LIMIT 50",
            params={"run_id": run_id},
        )
    ).to_pandas()
    session_table = mo.ui.table(
        session_ranking, selection="single", initial_selection=[0]
    )
    session_table
    return session_ranking, session_table


@app.cell
def session(mo, session_ranking, session_table):
    session = (
        session_table.value.iloc[0]
        if len(session_table.value)
        else session_ranking.iloc[0]
    )
    mo.md(
        f"Session `{session.session_id}`: {session.false_alarms:,} false alarms "
        f"over {session.hours:.1f} hours"
    )
    return (session,)


@app.cell
def session_path(gpd, lookout_medallion, pa, session):
    session_path = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, geometry FROM train_session_v3 WHERE session_id = $session_id",
                params={"session_id": session.session_id},
                country="DE",
            )
        )
    )
    session_path.assign(points=session_path.geometry.count_coordinates())[
        ["session_id", "points"]
    ]
    return (session_path,)


@app.cell
def session_transitions(lookout_medallion, pa, run_id, session):
    session_transitions = pa.table(
        lookout_medallion.query_gold(
            'SELECT crossing_compact_id, at, "from", "to" FROM evaluation_transition_v1 '
            "WHERE run_id = $run_id AND session_id = $session_id ORDER BY at, crossing_compact_id",
            params={"run_id": run_id, "session_id": session.session_id},
        )
    ).to_pandas()
    session_transitions.groupby("to").size().rename("transitions")
    return (session_transitions,)


@app.cell
def session_outcomes(lookout_medallion, pa, run_id, session):
    session_outcomes = pa.table(
        lookout_medallion.query_gold(
            "SELECT crossing_compact_id, outcome, class, notified_at, crossed_at, lead_ms, score "
            "FROM evaluation_outcome_v1 WHERE run_id = $run_id AND session_id = $session_id",
            params={"run_id": run_id, "session_id": session.session_id},
        )
    ).to_pandas()
    session_outcomes.groupby(["outcome", "class"], dropna=False).size().rename(
        "notifications"
    )
    return (session_outcomes,)


@app.cell
def session_crossings(crossings, session_outcomes, session_transitions):
    session_crossings = (
        crossings[
            crossings.crossing_compact_id.isin(
                session_transitions.crossing_compact_id
            )
        ]
        .merge(
            session_outcomes.groupby("crossing_compact_id")
            .outcome.agg(lambda _outcomes: " + ".join(sorted(set(_outcomes))))
            .rename("outcomes")
            .reset_index(),
            on="crossing_compact_id",
            how="left",
            validate="one_to_one",
        )
        .fillna({"outcomes": "candidate only"})
    )
    session_crossings.groupby("outcomes").size().rename("crossings")
    return (session_crossings,)


@app.cell
def session_samples(gpd, lookout_medallion, pa, session):
    session_samples = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT t, geometry FROM train_session_sample_v3 "
                "WHERE session_id = $session_id ORDER BY t",
                params={"session_id": session.session_id},
                country="DE",
            )
        )
    )
    session_samples.t.agg(["count", "min", "max"])
    return (session_samples,)


@app.cell
def session_alerts(crossings, gpd, session_outcomes, session_samples):
    _alerts = (
        session_outcomes.dropna(subset="notified_at")
        .merge(
            session_samples.rename(
                columns={"t": "notified_at", "geometry": "train"}
            ),
            on="notified_at",
            validate="many_to_one",
        )
        .merge(
            crossings[["crossing_compact_id", "geometry"]].rename(
                columns={"geometry": "crossing"}
            ),
            on="crossing_compact_id",
            validate="many_to_one",
        )
    )
    session_alerts = (
        gpd.GeoDataFrame(
            _alerts.assign(
                crossing=gpd.GeoSeries(_alerts.crossing, crs=crossings.crs)
            ),
            geometry=gpd.GeoSeries(_alerts.train, crs=crossings.crs),
        )
        .drop(columns="train")
        .rename_geometry("train")
    )
    _utm = session_alerts.estimate_utm_crs()
    session_alerts.assign(
        metres=session_alerts.train.to_crs(_utm).distance(
            session_alerts.crossing.to_crs(_utm)
        )
    ).groupby("outcome").metres.describe()[["count", "min", "50%", "max"]]
    return (session_alerts,)


@app.cell
def session_map(lonboard, pa, session_alerts, session_crossings, session_path):
    outcomes_colours = {
        "credited": [46, 160, 67],
        "credited + false_alarm": [235, 170, 52],
        "credited + false_alarm + repeat": [200, 120, 40],
        "credited + repeat": [130, 80, 223],
        "repeat": [130, 80, 223],
        "false_alarm": [207, 34, 46],
        "unscored": [137, 135, 129],
        "candidate only": [190, 190, 190],
    }
    alert_position_colour = [33, 33, 33]
    _alert_colours = lonboard.colormap.apply_categorical_cmap(
        session_alerts.outcome.astype(str),
        {
            _outcome: outcomes_colours[_outcome]
            for _outcome in session_alerts.outcome.unique()
        },
    )
    lonboard.Map(
        [
            lonboard.PathLayer.from_geopandas(
                session_path, get_color=[42, 120, 214], width_min_pixels=2
            ),
            lonboard.ArcLayer(
                pa.table(
                    {
                        "crossing_compact_id": session_alerts.crossing_compact_id,
                        "outcome": session_alerts.outcome.astype(str),
                        "notified_at": session_alerts.notified_at,
                    }
                ),
                get_source_position=session_alerts.train.to_arrow(
                    geometry_encoding="geoarrow"
                ),
                get_target_position=session_alerts.crossing.to_arrow(
                    geometry_encoding="geoarrow"
                ),
                get_source_color=_alert_colours,
                get_target_color=_alert_colours,
                get_tilt=90,
                width_min_pixels=2,
            ),
            lonboard.ScatterplotLayer.from_geopandas(
                session_alerts.drop_duplicates("notified_at")[
                    ["notified_at", "train"]
                ],
                get_fill_color=alert_position_colour,
                radius_min_pixels=3,
            ),
            lonboard.ScatterplotLayer.from_geopandas(
                session_crossings[["crossing_id", "outcomes", "geometry"]],
                get_fill_color=lonboard.colormap.apply_categorical_cmap(
                    session_crossings.outcomes.astype(str),
                    {
                        _outcomes: outcomes_colours.get(_outcomes, [0, 0, 0])
                        for _outcomes in session_crossings.outcomes.unique()
                    },
                ),
                radius_min_pixels=5,
            ),
        ],
        basemap=lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        ),
    )
    return alert_position_colour, outcomes_colours


@app.cell
def session_map_key(
    alert_position_colour,
    outcomes_colours,
    plt,
    session_alerts,
    session_crossings,
):
    _fig = plt.figure(figsize=(9, 1.6))
    _fig.legend(
        handles=[
            plt.Line2D(
                [],
                [],
                color=[42 / 255, 120 / 255, 214 / 255],
                label="train path",
            )
        ]
        + [
            plt.Line2D(
                [],
                [],
                color=[_c / 255 for _c in outcomes_colours[_outcome]],
                label=f"alert: {_outcome}",
            )
            for _outcome in sorted(session_alerts.outcome.unique())
        ]
        + [
            plt.Line2D(
                [],
                [],
                color=[_c / 255 for _c in alert_position_colour],
                marker="o",
                markersize=4,
                linestyle="",
                label="train, where it alerted",
            )
        ]
        + [
            plt.Line2D(
                [],
                [],
                color=[
                    _c / 255
                    for _c in outcomes_colours.get(_outcomes, [0, 0, 0])
                ],
                marker="o",
                linestyle="",
                label=f"crossing: {_outcomes}",
            )
            for _outcomes in sorted(session_crossings.outcomes.unique())
        ],
        loc="center",
        ncols=3,
        frameon=False,
    )
    _fig
    return


@app.cell
def session_timeline(plt, session, session_outcomes, session_transitions):
    state_colours = {
        "candidate_pass": "#bebebe",
        "alerted_pass": "#eb6834",
        "actual_pass": "#2ea043",
    }
    _order = (
        session_transitions.groupby("crossing_compact_id")
        .at.min()
        .sort_values()
        .index.tolist()
    )
    _rows = {_crossing: _row for _row, _crossing in enumerate(_order)}
    _figure, _axes = plt.subplots(figsize=(12, max(3, len(_order) * 0.12)))
    for _crossing, _steps in session_transitions.groupby(
        "crossing_compact_id"
    ):
        _starts = _steps["at"].tolist()
        _ends = _starts[1:] + [session.ended_at]
        for _start, _end, _state in zip(_starts, _ends, _steps["to"]):
            if _state in state_colours:
                _axes.barh(
                    _rows[_crossing],
                    _end - _start,
                    left=_start,
                    height=0.8,
                    color=state_colours[_state],
                )
    _passes = session_outcomes.dropna(subset=["crossed_at"])
    _axes.scatter(
        _passes.crossed_at,
        _passes.crossing_compact_id.map(_rows),
        marker="|",
        color="black",
        s=40,
        zorder=3,
    )
    _axes.set_yticks([])
    _axes.set_ylabel("crossing, in order of first transition")
    _axes.set_xlabel("time")
    _axes.legend(
        handles=[
            plt.Rectangle((0, 0), 1, 1, color=_colour, label=_state)
            for _state, _colour in state_colours.items()
        ]
        + [
            plt.Line2D(
                [], [], marker="|", color="black", linestyle="", label="pass"
            )
        ],
        loc="upper left",
    )
    _figure.tight_layout()
    _axes
    return


if __name__ == "__main__":
    app.run()
