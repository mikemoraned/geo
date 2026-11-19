# /// script
# dependencies = [
#     "geographiclib==2.0",
#     "lonboard==0.17.0",
#     "lookout-evaluation",
#     "lookout-medallion",
#     "marimo>=0.25.2",
#     "matplotlib==3.11.2",
#     "numpy==2.5.4",
#     "polars==2.1.0",
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


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    # LightCone as CrowFlies with a bearing filter

    LightCone now runs CrowFlies, and lowers each crossing CrowFlies alerts outside its cone to low
        confidence, which raises no alert. The
    cone is centred on the heading `LastMove` estimates from the samples, and spans the half-width
    either side of it. At a half-width of 180° LightCone alerts exactly as CrowFlies does. CrowFlies
    itself now takes its speed from `LastMove` rather than from the receiver.

    The runs of 2026-11-16 score CrowFlies at 1 km, and LightCone at 1 km with half-widths of 15°, 30°,
    45°, 60°, 75°, and 90°. Each runs over all 34,521 sessions with at least 90% realtime, scored on the
    tune split. This notebook compares LightCone at one half-width with CrowFlies: by outcome, by pass,
    by session, by speed, by crossing, and in example sessions drawn with the cone at each alert.
    """)
    return


@app.cell
def _():
    import lonboard
    from geographiclib.geodesic import Geodesic
    import marimo as mo
    import matplotlib.pyplot as plt
    import numpy as np
    import polars as pl
    import pyarrow as pa
    import shapely

    import lookout_evaluation as evaluation
    import lookout_medallion

    return (
        Geodesic,
        evaluation,
        lonboard,
        lookout_medallion,
        mo,
        np,
        pa,
        pl,
        plt,
        shapely,
    )


@app.cell
def _(lookout_medallion, pa, pl):
    sweep = (
        pl.from_arrow(
            pa.table(
                lookout_medallion.query_gold(
                    "SELECT r.run_id, r.predictor, r.predictor_settings, s.sessions, s.passes, "
                    "s.notifications, s.precision, s.precision_low, s.precision_high, s.recall, "
                    "s.recall_low, s.recall_high, s.false_alarms_per_hour, s.false_alarms_per_hour_low, "
                    "s.false_alarms_per_hour_high, s.repeats, s.misses, s.lead_median "
                    "FROM evaluation_run_v6 r JOIN evaluation_summary_v3 s USING (run_id) "
                    "WHERE r.run_date = '2026-11-16'"
                )
            )
        )
        .filter(
            pl.col("predictor_settings").map.get("radius_metres") == "1000",
            pl.col("predictor_settings")
            .map.get("still_within_metres")
            .is_not_null(),
        )
        .with_columns(
            half_width=pl.col("predictor_settings")
            .map.get("half_width_degrees")
            .cast(pl.Int32)
        )
        .drop("predictor_settings")
        .sort("half_width", nulls_last=True)
    )
    sweep
    return (sweep,)


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## Each half-width trades recall for false alarms

    Each dot is one run, with whiskers for its 95% intervals on both measures. LightCone's dots run
    from 15°, at the bottom left, to 90°, at the top right. CrowFlies sits far to the right, at the
    recall of LightCone's widest cones.
    """)
    return


@app.cell
def _(pl, plt, sweep):
    _light_cone = sweep.filter(pl.col("predictor") == "light_cone")
    _crow_flies = sweep.filter(pl.col("predictor") == "crow_flies")
    _fig, _ax = plt.subplots(figsize=(9, 4))
    for _runs, _colour, _label in (
        (_light_cone, "#2a6fdb", "LightCone"),
        (_crow_flies, "#e8850c", "CrowFlies 1 km"),
    ):
        _ax.errorbar(
            _runs["false_alarms_per_hour"],
            _runs["recall"],
            xerr=[
                _runs["false_alarms_per_hour"]
                - _runs["false_alarms_per_hour_low"],
                _runs["false_alarms_per_hour_high"]
                - _runs["false_alarms_per_hour"],
            ],
            yerr=[
                _runs["recall"] - _runs["recall_low"],
                _runs["recall_high"] - _runs["recall"],
            ],
            fmt="o-" if _label == "LightCone" else "o",
            color=_colour,
            linewidth=2,
            markersize=6,
            label=_label,
        )
    for _run in _light_cone.iter_rows(named=True):
        _ax.annotate(
            f"{_run['half_width']}°",
            (_run["false_alarms_per_hour"], _run["recall"]),
            textcoords="offset points",
            xytext=(6, -12),
        )
    _ax.set_xscale("log")
    _ax.set_xlabel("false alarms per hour (log scale)")
    _ax.set_ylabel("recall")
    _ax.legend(frameon=False, loc="lower right")
    _ax.spines[["top", "right"]].set_visible(False)
    _fig
    return


@app.cell
def _(mo, pl, sweep):
    light_cone_choice = mo.ui.dropdown(
        options={
            f"{_run['half_width']}°": _run["run_id"]
            for _run in sweep.filter(
                pl.col("predictor") == "light_cone"
            ).iter_rows(named=True)
        },
        value="45°",
        label="LightCone's half-width",
    )
    light_cone_choice
    return (light_cone_choice,)


@app.cell
def _(light_cone_choice, pl, sweep):
    compared = {
        "light_cone": light_cone_choice.value,
        "crow_flies": sweep.filter(pl.col("predictor") == "crow_flies")[
            "run_id"
        ].item(),
    }
    predictor_names = {
        "light_cone": "LightCone",
        "crow_flies": "CrowFlies 1 km",
    }
    predictor_colours = {"light_cone": "#2a6fdb", "crow_flies": "#e8850c"}
    compared
    return compared, predictor_colours, predictor_names


@app.cell
def _(compared, lookout_medallion, pa, pl):
    outcomes = pl.concat(
        [
            pl.from_arrow(
                pa.table(
                    lookout_medallion.query_gold(
                        "SELECT session_id, crossing_compact_id, outcome, class, notified_at, "
                        "crossed_at, lead, score FROM evaluation_outcome_v2 WHERE run_id = $run_id",
                        params={"run_id": _run_id},
                    )
                )
            ).with_columns(predictor=pl.lit(_predictor))
            for _predictor, _run_id in compared.items()
        ]
    )
    outcomes.pivot(
        on="predictor",
        index=["outcome", "class"],
        values="session_id",
        aggregate_function="len",
    ).sort("outcome", "class")
    return (outcomes,)


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## What the filter does to each pass

    A pass credits the notification before it that scores worst, and counts the others as repeats. Each
    pass gets the class of its credited notification under each predictor, or miss if none came. The
    table counts the passes in each pair of classes, with CrowFlies' class in the rows.

    LightCone notifies on a subset of what CrowFlies alerts on. Most passes therefore keep their class
    or fall to a worse one. A few rise. In those, CrowFlies alerted early, dropped the crossing, and
    alerted again at the last moment. LightCone dropped that second alert: by then the crossing lay
    within a few hundred metres, beside or behind the train, and outside the cone.
    """)
    return


@app.cell
def _(outcomes, pl):
    _pass = ["session_id", "crossing_compact_id", "crossed_at"]
    pass_classes = (
        outcomes.filter(pl.col("outcome").is_in(["credited", "miss"]))
        .with_columns(pass_class=pl.coalesce("class", pl.col("outcome")))
        .pivot(on="predictor", index=_pass, values="pass_class")
    )
    pass_classes.group_by("crow_flies", "light_cone").len().pivot(
        on="light_cone", index="crow_flies", values="len"
    ).sort("crow_flies")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## Lead times

    The chart places each credited pass by the lead time of its first alert. The dashed lines mark
    the last moment, at 30 s, and the horizon, at 5 minutes.
    """)
    return


@app.cell
def _(np, outcomes, pl, plt, predictor_colours, predictor_names):
    _credited = outcomes.filter(pl.col("outcome") == "credited")
    _bins = np.arange(0, 421, 10)
    _fig, _ax = plt.subplots(figsize=(9, 3.5))
    for _predictor, _colour in predictor_colours.items():
        _ax.hist(
            _credited.filter(pl.col("predictor") == _predictor)[
                "lead"
            ].dt.total_seconds(),
            bins=_bins,
            histtype="step",
            linewidth=2,
            color=_colour,
            label=predictor_names[_predictor],
        )
    for _seconds in (30, 300):
        _ax.axvline(_seconds, color="grey", linestyle="--", linewidth=1)
    _ax.set_xlabel("lead time of the first alert before a pass (s)")
    _ax.set_ylabel("credited passes")
    _ax.legend(frameon=False)
    _ax.spines[["top", "right"]].set_visible(False)
    _fig
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## How the two predictors spread across sessions

    Each session gets the summary's measures, from its own outcomes: recall is the credited score over
    its passes, and false alarms per hour divides its false alarms by its scored duration. A session's
    mean speed is the length of its track over its duration, dwelling included. The comparison keeps
    the sessions with at least 5 passes and 30 minutes of scored time, since fewer make the ratios
    jump between extremes.
    """)
    return


@app.cell
def _(compared, lookout_medallion, pa, pl):
    scored_hours = (
        pl.from_arrow(
            pa.table(
                lookout_medallion.query_gold(
                    "SELECT session_id, scored_duration FROM evaluation_session_v2 WHERE run_id = $run_id",
                    params={"run_id": compared["crow_flies"]},
                )
            )
        )
        .filter(pl.col("scored_duration") > pl.duration(seconds=0))
        .select(
            "session_id",
            hours=pl.col("scored_duration").dt.total_seconds() / 3600,
        )
    )
    scored_hours.describe()
    return (scored_hours,)


@app.cell
def _(lookout_medallion, pa, pl, shapely):
    _tracks = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, geometry_projected, started_at, ended_at FROM train_session_v3",
                country="DE",
            )
        )
    )
    mean_speeds = _tracks.select(
        "session_id",
        mean_kmh=pl.Series(
            shapely.length(
                shapely.from_wkb(_tracks["geometry_projected"].to_list())
            )
            / 1000
        )
        / (
            (pl.col("ended_at") - pl.col("started_at")).dt.total_seconds()
            / 3600
        ),
    )
    mean_speeds.describe()
    return (mean_speeds,)


@app.cell
def _(outcomes, pl):
    session_tallies = outcomes.group_by("predictor", "session_id").agg(
        credited=pl.col("score").filter(pl.col("outcome") == "credited").sum(),
        passes=pl.col("outcome")
        .is_in(["credited", "miss"])
        .sum()
        .cast(pl.Int64),
        false_alarms=(pl.col("outcome") == "false_alarm").sum().cast(pl.Int64),
        repeats=(pl.col("outcome") == "repeat").sum().cast(pl.Int64),
    )
    session_tallies.group_by("predictor").agg(
        pl.len(), pl.col("passes", "false_alarms").sum()
    )
    return (session_tallies,)


@app.cell
def _(mean_speeds, mo, pl, scored_hours, session_tallies):
    def _of(predictor, suffix):
        return (
            session_tallies.filter(pl.col("predictor") == predictor)
            .drop("predictor")
            .rename(
                lambda _column: (
                    _column
                    if _column == "session_id"
                    else f"{_column}_{suffix}"
                )
            )
        )

    session_measures = (
        scored_hours.join(_of("light_cone", "lc"), on="session_id", how="left")
        .join(_of("crow_flies", "cf"), on="session_id", how="left")
        .fill_null(0)
        .join(mean_speeds, on="session_id", how="left")
        .with_columns(passes=pl.col("passes_cf"))
        .with_columns(
            recall_lc=pl.col("credited_lc") / pl.col("passes"),
            recall_cf=pl.col("credited_cf") / pl.col("passes"),
            false_alarms_per_hour_lc=pl.col("false_alarms_lc")
            / pl.col("hours"),
            false_alarms_per_hour_cf=pl.col("false_alarms_cf")
            / pl.col("hours"),
        )
        .with_columns(
            recall_gain=pl.col("recall_lc") - pl.col("recall_cf"),
            false_alarms_per_hour_gain=pl.col("false_alarms_per_hour_lc")
            - pl.col("false_alarms_per_hour_cf"),
        )
        .drop("passes_lc", "passes_cf")
    )
    compared_sessions = session_measures.filter(
        pl.col("passes") >= 5, pl.col("hours") >= 0.5
    )
    mo.md(
        f"{session_measures.height:,} sessions with scored time, of which "
        f"{compared_sessions.height:,} have at least 5 passes and 30 minutes"
    )
    return (compared_sessions,)


@app.cell
def _(compared_sessions, np, plt, predictor_colours, predictor_names):
    def _ecdf(_ax, _column, _label):
        for _predictor, _suffix in (
            ("light_cone", "lc"),
            ("crow_flies", "cf"),
        ):
            _values = np.sort(
                compared_sessions[f"{_column}_{_suffix}"].to_numpy()
            )
            _ax.plot(
                _values,
                np.arange(1, len(_values) + 1) / len(_values),
                color=predictor_colours[_predictor],
                linewidth=2,
                label=predictor_names[_predictor],
            )
        _ax.set_xlabel(_label)
        _ax.spines[["top", "right"]].set_visible(False)

    _fig, (_recall, _false_alarms) = plt.subplots(
        1, 2, figsize=(10, 3.5), sharey=True
    )
    _ecdf(_recall, "recall", "recall per session")
    _ecdf(
        _false_alarms,
        "false_alarms_per_hour",
        "false alarms per hour per session",
    )
    _false_alarms.set_xlim(0, 150)
    _recall.set_ylabel("share of sessions at or below")
    _recall.legend(frameon=False)
    _fig
    return


@app.cell
def _(compared_sessions, mo, np, pl, plt):
    _fig, (_recall, _false_alarms) = plt.subplots(1, 2, figsize=(10, 3.5))
    _recall.hist(
        compared_sessions["recall_gain"],
        bins=np.linspace(-1, 1, 41),
        color="#5b6770",
    )
    _recall.set_xlabel("recall: LightCone − CrowFlies")
    _false_alarms.hist(
        compared_sessions["false_alarms_per_hour_gain"].clip(-100, 100),
        bins=np.linspace(-100, 100, 41),
        color="#5b6770",
    )
    _false_alarms.set_xlabel(
        "false alarms per hour: LightCone − CrowFlies (clipped to ±100)"
    )
    _recall.set_ylabel("sessions")
    for _ax in (_recall, _false_alarms):
        _ax.axvline(0, color="black", linewidth=1)
        _ax.spines[["top", "right"]].set_visible(False)
    mo.vstack(
        [
            _fig,
            compared_sessions.select(
                pl.col("recall_gain", "false_alarms_per_hour_gain")
                .quantile(_q)
                .name.suffix(f"_p{round(_q * 100)}")
                for _q in (0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99)
            ).unpivot(),
            mo.md(
                f"LightCone has the higher recall in "
                f"{(compared_sessions['recall_gain'] > 0).sum():,} sessions, and fewer false alarms "
                f"per hour in {(compared_sessions['false_alarms_per_hour_gain'] < 0).sum():,}, of "
                f"{compared_sessions.height:,}"
            ),
        ]
    )
    return


@app.cell
def _(compared_sessions, mo, pl, plt, predictor_colours, predictor_names):
    _by_speed = (
        compared_sessions.with_columns(
            speed_band=(pl.col("mean_kmh") // 20 * 20)
            .clip(0, 160)
            .cast(pl.Int32)
        )
        .group_by("speed_band")
        .agg(
            sessions=pl.len(),
            recall_lc=pl.col("credited_lc").sum() / pl.col("passes").sum(),
            recall_cf=pl.col("credited_cf").sum() / pl.col("passes").sum(),
            false_alarms_per_hour_lc=pl.col("false_alarms_lc").sum()
            / pl.col("hours").sum(),
            false_alarms_per_hour_cf=pl.col("false_alarms_cf").sum()
            / pl.col("hours").sum(),
        )
        .sort("speed_band")
    )
    _fig, (_recall, _false_alarms) = plt.subplots(1, 2, figsize=(10, 3.5))
    for _predictor, _suffix in (("light_cone", "lc"), ("crow_flies", "cf")):
        for _ax, _column in (
            (_recall, "recall"),
            (_false_alarms, "false_alarms_per_hour"),
        ):
            _ax.plot(
                _by_speed["speed_band"],
                _by_speed[f"{_column}_{_suffix}"],
                marker="o",
                linewidth=2,
                color=predictor_colours[_predictor],
                label=predictor_names[_predictor],
            )
    _recall.set_ylabel("recall")
    _false_alarms.set_ylabel("false alarms per hour")
    for _ax in (_recall, _false_alarms):
        _ax.set_xlabel("mean speed of the session, in bands of 20 km/h")
        _ax.spines[["top", "right"]].set_visible(False)
    _recall.legend(frameon=False)
    mo.vstack([mo.md("### By the session's mean speed"), _fig, _by_speed])
    return


@app.cell
def _(lookout_medallion, mo, pa, pl, shapely):
    crossings = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT crossing_compact_id, crossing_id, geometry FROM water_crossing_v2",
                country="DE",
            )
        )
    )
    geometry_type = crossings.schema["geometry"]
    _points = shapely.from_wkb(crossings["geometry"].to_list())
    crossings = crossings.with_columns(
        lon=pl.Series(shapely.get_x(_points)),
        lat=pl.Series(shapely.get_y(_points)),
    )
    mo.md(f"{crossings.height:,} crossings in DE")
    return crossings, geometry_type


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## Where LightCone keeps false alarms, by crossing

    Each crossing with a false alarm under either predictor is a dot, sized by CrowFlies' false alarms
    there and coloured by the share LightCone keeps: blue where the filter removes them all, red where
    it keeps them all. Large red dots are the crossings the cone cannot tell apart from the line ahead.
    """)
    return


@app.cell
def _(crossings, outcomes, pl):
    false_alarms_by_crossing = (
        outcomes.filter(pl.col("outcome") == "false_alarm")
        .group_by("crossing_compact_id")
        .agg(
            light_cone=(pl.col("predictor") == "light_cone")
            .sum()
            .cast(pl.Int64),
            crow_flies=(pl.col("predictor") == "crow_flies")
            .sum()
            .cast(pl.Int64),
        )
        .with_columns(kept=pl.col("light_cone") / pl.col("crow_flies"))
        .join(crossings, on="crossing_compact_id")
        .sort("light_cone", descending=True)
    )
    false_alarms_by_crossing.select(
        pl.len(),
        pl.col("light_cone", "crow_flies").sum(),
        (pl.col("light_cone") > pl.col("crow_flies"))
        .sum()
        .alias("crossings_where_light_cone_raises_more"),
    )
    return (false_alarms_by_crossing,)


@app.cell
def _(false_alarms_by_crossing, lonboard, mo, np, plt):
    _columns = [
        "crossing_id",
        "light_cone",
        "crow_flies",
        "kept",
        "lon",
        "lat",
    ]
    mo.vstack(
        [
            lonboard.Map(
                lonboard.ScatterplotLayer(
                    table=false_alarms_by_crossing.select(
                        "crossing_id",
                        "light_cone",
                        "crow_flies",
                        "kept",
                        "geometry",
                    ).to_arrow(),
                    get_fill_color=lonboard.colormap.apply_continuous_cmap(
                        false_alarms_by_crossing["kept"].clip(0, 1).to_numpy(),
                        plt.get_cmap("RdBu_r"),
                    ),
                    get_radius=np.sqrt(
                        false_alarms_by_crossing["crow_flies"].to_numpy()
                    )
                    * 40,
                    radius_min_pixels=1.5,
                ),
                basemap=lonboard.basemap.MaplibreBasemap(
                    style=lonboard.basemap.CartoStyle.Positron
                ),
            ),
            mo.md("Crossings where LightCone keeps the most false alarms"),
            false_alarms_by_crossing.select(_columns).head(10),
        ]
    )
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## Sessions picked from the spread

    Rank the compared sessions by one difference between the predictors, and pick the session at each
    of the worst, p10, p25, the median, p75, p90, and the best, for LightCone. Selecting a row draws
    that session below.
    """)
    return


@app.cell
def _(mo):
    ranking = mo.ui.dropdown(
        options={
            "recall: LightCone − CrowFlies": "recall_gain",
            "false alarms per hour: CrowFlies − LightCone": "false_alarms_per_hour_gain",
        },
        value="recall: LightCone − CrowFlies",
        label="Rank sessions by",
    )
    ranking
    return (ranking,)


@app.cell
def _(compared_sessions, mo, pl, ranking):
    _ranked = compared_sessions.sort(
        ranking.value, descending=ranking.value == "false_alarms_per_hour_gain"
    )
    _picks = {
        "worst": 0.0,
        "p10": 0.1,
        "p25": 0.25,
        "median": 0.5,
        "p75": 0.75,
        "p90": 0.9,
        "best": 1.0,
    }
    examples = mo.ui.table(
        pl.concat(
            [
                _ranked.slice(
                    round(_q * (_ranked.height - 1)), 1
                ).with_columns(pick=pl.lit(_pick))
                for _pick, _q in _picks.items()
            ]
        ).select(
            "pick",
            "session_id",
            pl.col("hours").round(2),
            pl.col("mean_kmh").round(0),
            "passes",
            pl.col("recall_lc", "recall_cf").round(2),
            pl.col(
                "false_alarms_per_hour_lc", "false_alarms_per_hour_cf"
            ).round(1),
            "repeats_lc",
            "repeats_cf",
        ),
        selection="single",
        initial_selection=[3],
    )
    examples
    return (examples,)


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## One session

    Each map shows one predictor in the session. The track is grey, and a black dot marks where the
    train was each time it alerted. An arc leaves that dot for each crossing the alert named, coloured
    by what came of the alert: a hit, too early, at the last moment, a false alarm, or a repeat of an
    earlier alert. Each crossing is a dot coloured by its best outcome, with black for a pass no
    alert announced. The table under each map counts that predictor's alerts and misses.

    LightCone's map adds the cone at each of its alerts, in translucent blue. Each cone spans the
    half-width either side of the heading `LastMove` estimated at that sample, out to 1 km.
    """)
    return


@app.cell
def _(examples, lookout_medallion, mo, pa, pl):
    example = (
        examples.value.row(0, named=True)
        if examples.value.height
        else examples.data.row(3, named=True)
    )
    session_samples = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, t, seq, lon, lat FROM train_session_sample_v3 "
                "WHERE session_id = $session_id ORDER BY t, seq",
                params={"session_id": example["session_id"]},
                country="DE",
            )
        )
    )
    mo.md(
        f"Session `{example['session_id']}` ({example['pick']}): {example['hours']} hours at "
        f"{example['mean_kmh']:.0f} km/h, {example['passes']} passes, "
        f"{session_samples.height:,} samples"
    )
    return example, session_samples


@app.cell
def _(evaluation, pa, pl, session_samples):
    session_velocities = pl.from_arrow(
        pa.table(
            evaluation.velocities(
                samples=session_samples.to_arrow(), still_within_metres=20
            )
        )
    ).unique("t", keep="last", maintain_order=True)
    session_velocities.select(
        pl.len(),
        pl.col("heading_degrees").null_count().alias("without_heading"),
    )
    return (session_velocities,)


@app.cell
def _(
    Geodesic,
    compared,
    example,
    geometry_type,
    mo,
    np,
    outcomes,
    pl,
    session_samples,
    session_velocities,
    shapely,
    sweep,
):
    def _cone(lon, lat, heading, half_width, radius_metres=1000, steps=24):
        _edge = [
            Geodesic.WGS84.Direct(lat, lon, _azimuth, radius_metres)
            for _azimuth in np.linspace(
                heading - half_width, heading + half_width, steps + 1
            )
        ]
        return shapely.Polygon(
            [(lon, lat), *((_end["lon2"], _end["lat2"]) for _end in _edge)]
        )

    half_width = sweep.filter(pl.col("run_id") == compared["light_cone"])[
        "half_width"
    ].item()
    _alerted = (
        outcomes.filter(
            pl.col("predictor") == "light_cone",
            pl.col("session_id") == example["session_id"],
            pl.col("notified_at").is_not_null(),
        )
        .select(t="notified_at")
        .unique()
        .join(session_velocities, on="t")
        .join(
            session_samples.unique("t", keep="last").select("t", "lon", "lat"),
            on="t",
        )
        .drop_nulls("heading_degrees")
        .sort("t")
    )
    cones = _alerted.select(
        "t",
        pl.col("heading_degrees").round(1),
        pl.Series(
            "geometry",
            shapely.to_wkb(
                [
                    _cone(
                        _row["lon"],
                        _row["lat"],
                        _row["heading_degrees"],
                        half_width,
                    )
                    for _row in _alerted.iter_rows(named=True)
                ]
            ),
        ).cast(geometry_type),
    )
    mo.md(
        f"{cones.height} cones of ±{half_width}° at LightCone's alerts in this session"
    )
    return (cones,)


@app.cell
def _(
    cones,
    crossings,
    example,
    geometry_type,
    lonboard,
    mo,
    np,
    outcomes,
    pl,
    predictor_names,
    session_samples,
    shapely,
):
    _outcome_colours = {
        "hit": [46, 160, 67],
        "too early": [130, 80, 223],
        "last moment": [235, 170, 52],
        "miss": [20, 20, 20],
        "false alarm": [207, 34, 46],
        "repeat": [137, 135, 129],
    }
    _rank = {
        _kind: _position for _position, _kind in enumerate(_outcome_colours)
    }
    _kind = (
        pl.when(pl.col("outcome") == "credited")
        .then(pl.col("class").str.replace("_", " "))
        .when(pl.col("outcome") == "false_alarm")
        .then(pl.lit("false alarm"))
        .otherwise(pl.col("outcome"))
    )

    def _geometries(geometries):
        return pl.Series("geometry", shapely.to_wkb(geometries)).cast(
            geometry_type
        )

    def _colours(kinds):
        return np.array([_outcome_colours[_k] for _k in kinds], dtype=np.uint8)

    _track = pl.DataFrame(
        [
            _geometries(
                [
                    shapely.LineString(
                        session_samples.select("lon", "lat").to_numpy()
                    )
                ]
            )
        ]
    )

    def _session_map(predictor):
        _outcomes = outcomes.filter(
            pl.col("predictor") == predictor,
            pl.col("session_id") == example["session_id"],
            pl.col("outcome") != "unscored",
        ).with_columns(kind=_kind)
        _crossings = (
            _outcomes.with_columns(rank=pl.col("kind").replace_strict(_rank))
            .sort("rank")
            .group_by("crossing_compact_id")
            .first()
            .select("crossing_compact_id", "kind")
            .join(crossings, on="crossing_compact_id")
        )
        _alerts = (
            _outcomes.filter(pl.col("notified_at").is_not_null())
            .join(
                session_samples.unique("t").rename(
                    {"lon": "train_lon", "lat": "train_lat"}
                ),
                left_on="notified_at",
                right_on="t",
            )
            .join(
                crossings.select("crossing_compact_id", "lon", "lat"),
                on="crossing_compact_id",
            )
        )
        _alert_colours = _colours(_alerts["kind"])
        _alerted_from = _alerts.unique("notified_at").with_columns(
            _geometries(
                shapely.points(
                    _alerts.unique("notified_at")
                    .select("train_lon", "train_lat")
                    .to_numpy()
                )
            )
        )
        _layers = [
            lonboard.PathLayer(
                table=_track.to_arrow(),
                get_color=[120, 120, 120],
                width_min_pixels=2,
            ),
            lonboard.ScatterplotLayer(
                table=_crossings.select(
                    "crossing_id", "kind", "geometry"
                ).to_arrow(),
                get_fill_color=_colours(_crossings["kind"]),
                radius_min_pixels=4,
            ),
        ]
        if predictor == "light_cone" and cones.height:
            _layers.insert(
                1,
                lonboard.PolygonLayer(
                    table=cones.to_arrow(),
                    get_fill_color=[42, 111, 219, 40],
                    get_line_color=[42, 111, 219, 160],
                    line_width_min_pixels=1,
                ),
            )
        if _alerts.height:
            _layers[1:1] = [
                lonboard.ArcLayer(
                    table=_alerts.select(
                        "crossing_compact_id", "kind", "notified_at", "lead"
                    ).to_arrow(),
                    get_source_position=_alerts.select(
                        "train_lon", "train_lat"
                    ).to_numpy(),
                    get_target_position=_alerts.select(
                        "lon", "lat"
                    ).to_numpy(),
                    get_source_color=_alert_colours,
                    get_target_color=_alert_colours,
                    get_tilt=90,
                    width_min_pixels=2,
                ),
                lonboard.ScatterplotLayer(
                    table=_alerted_from.select(
                        "notified_at", "geometry"
                    ).to_arrow(),
                    get_fill_color=[33, 33, 33],
                    radius_min_pixels=3,
                ),
            ]
        return mo.vstack(
            [
                mo.md(f"**{predictor_names[predictor]}**"),
                lonboard.Map(
                    _layers,
                    basemap=lonboard.basemap.MaplibreBasemap(
                        style=lonboard.basemap.CartoStyle.Positron
                    ),
                ),
                _outcomes.group_by("kind").len().sort("len", descending=True),
            ]
        )

    mo.vstack(
        [
            mo.md(
                "Colours: "
                + ", ".join(
                    f"<span style='color: rgb({_r},{_g},{_b})'>●</span> {_name}"
                    for _name, (_r, _g, _b) in _outcome_colours.items()
                )
            ),
            mo.hstack(
                [_session_map("light_cone"), _session_map("crow_flies")],
                widths="equal",
            ),
        ]
    )
    return


if __name__ == "__main__":
    app.run()
