# /// script
# dependencies = [
#     "geopandas==1.2.0",
#     "lonboard==0.17.0",
#     "lookout-evaluation",
#     "lookout-medallion",
#     "marimo>=0.25.2",
#     "matplotlib==3.11.2",
#     "numpy==2.5.4",
#     "pandas==3.1.1",
#     "polars==2.1.0",
#     "pya5==1.0.0",
#     "pyarrow==25.0.0",
#     "pyproj==3.7.2",
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
def intro(mo):
    mo.md("""
    # Where LightCone beats CrowFlies, and where it loses

    The sweep of 2026-11-16 ran LightCone and CrowFlies over all 34,521 sessions the filter keeps,
    scored on the tune split. This notebook compares one LightCone run with CrowFlies at 1 km. It
    shows the outcomes each predictor gives, how they spread across sessions, and sessions picked from
    that spread, drawn on a map.
    """)
    return


@app.cell(hide_code=True)
def findings(mo):
    mo.md("""
    ## Findings, for LightCone at 30°

    - **LightCone alerts early, and that alone costs it most of its recall.** It credits 174,791
      passes as too early, against 104,892 hits. CrowFlies at 1 km credits 6,299 as too early and
      244,491 as hits.
    - **Most early alerts come round again in time.** In 88% of the passes alerted too early, the
      crossing leaves the cone, enters it again, and the second alert falls within the horizon. That
      second alert counts as a repeat. Scored as a hit instead, LightCone's recall would reach 0.90.
    - **The median session favours LightCone on false alarms, the tail does not.** Over the 12,531
      sessions with 5 passes and 30 minutes, LightCone raises fewer false alarms per hour in 6,560.
      The median difference is 1.1 fewer an hour, but p90 is 35 more and p99 152 more. LightCone's
      recall is lower in all but 578 sessions, by a median of 0.48.
    - **Speed decides which predictor wins.** Below a mean of 40 km/h, LightCone raises fewer false
      alarms than CrowFlies. From 100 km/h, its recall is higher, 0.71 against 0.25 above 160 km/h.
      At speed, CrowFlies' 1 km leaves seconds of warning, and its alerts fall at the last moment.
    - **Stations by water show both effects.** Next to Köln Hbf, CrowFlies raises about 1,040 false
      alarms on each crossing of the Rhine just north of the station, and LightCone about 60. On the crossings a kilometre south, LightCone raises about 1,460
      and CrowFlies 12. On the Spree by Berlin Friedrichstraße, LightCone raises about 1,400 a
      crossing, and CrowFlies none, where the track runs beside the river.
    """)
    return


@app.cell
def how_heading(mo):
    mo.md("""
    ## How a run chooses what it counts

    A run makes four choices before it reports a number. Three of them each guard against one way the
    number could mislead. The pick guards against nothing: it trades precision for speed. Each step
    below makes one choice, set by the flags of `just evaluate` shown:

    1. **The filter** guards against scoring the predictor against the plan rather than the train. A
       session's positions are both the predictor's input and the ground truth, so the filter keeps
       the sessions at least 90% realtime: `--realtime-at-least 90`.
    2. **The pick** sets how many of the kept sessions a run replays: `--sessions 1000 --seed 1`, or
       `--sessions all`. Fewer sessions give a quicker run with wider intervals, and the seed keeps a
       small sample the same from run to run. A run that picks every session, as the sweep did, loses
       nothing to the pick.
    3. **The split** guards against reporting a number on the data used to choose the settings. It has
       two parts:
       - It assigns each place to the tune split or the test split, about 70% of places to tune:
         `--split-resolution 8 --split-salt tune-test-1 --tune-buckets 70`. Sessions share routes and
         crossings, so a split by session would put the same crossings in both. A split by place keeps
         each crossing in one.
       - A run counts only what happens in one split's places: `--split tune` while settings are
         chosen, then `--split test` once at the end. Every session replays in full, so the predictor
         keeps its history across places that do not count.
    4. **The bootstrap** guards against reading noise as a difference. It redraws the counted sessions
       to estimate how far each measure would move under another draw of sessions like these:
       `--resamples 1000 --bootstrap-seed 1`.
    """)
    return


@app.cell
def filter_heading(mo):
    mo.md("""
    ### Step 1: the filter

    Silver holds every DE train session. The filter keeps those whose positions are at least 90%
    realtime. A timetable's positions show where the train was planned to be, and realtime
    positions show where it was.
    """)
    return


@app.cell
def filtered(lookout_medallion, pa, pl):
    filtered = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT count(*) AS sessions, "
                "sum(CASE WHEN realtime_percentage >= 90 THEN 1 ELSE 0 END) AS kept "
                "FROM train_session_v3",
                country="DE",
            )
        )
    )
    filtered
    return (filtered,)


@app.cell
def pick_heading(mo):
    mo.md("""
    ### Step 2: the pick

    The pick chooses which of the kept sessions a run replays, at random under a seed. The sweep
    picked all of them, so its seed changed nothing. The earlier phases picked 1,000, to run faster.
    """)
    return


@app.cell
def picked(compared, evaluation, filtered, lookout_medallion, mo, pa, pl):
    picked = pl.from_arrow(
        pa.table(
            evaluation.pick(
                country="DE",
                realtime_at_least=90.0,
                sessions=filtered["kept"].item(),
                seed=1,
            )
        )
    ).select("session_id")
    _recorded = pl.from_arrow(
        pa.table(
            lookout_medallion.query_gold(
                "SELECT session_id FROM evaluation_session_v2 WHERE run_id = $run_id",
                params={"run_id": compared["crow_flies"]},
            )
        )
    )
    mo.md(
        f"The sweep's pick holds {picked.height:,} sessions, every one the filter kept. The sweep "
        f"recorded {_recorded.height:,} sessions, and "
        f"{_recorded.join(picked, on='session_id', how='semi').height:,} of them are in this pick."
    )
    return (picked,)


@app.cell
def split_heading(mo):
    mo.md("""
    ### Step 3: the split

    #### Assigning places

    The split divides places, not sessions. It cuts the map into A5 cells at resolution 8, each about
    520 km², some 25 km across. It hashes each cell's id with the salt `tune-test-1` into one of 100
    buckets. Buckets 0 to 69 make the tune split, and the other 30 the test split, so about 70% of
    cells are tune. A crossing falls in the split of the cell it lies in.
    """)
    return


@app.cell
def splitter_cell(crossings, evaluation, pa, pl):
    splitter = evaluation.Splitter(
        resolution=8, salt="tune-test-1", tune_buckets=70
    )
    crossing_splits = crossings.with_columns(
        split=pl.Series(
            splitter.splits(
                pa.array(crossings["lon"].to_numpy()),
                pa.array(crossings["lat"].to_numpy()),
            )
        )
    )
    crossing_splits.group_by("split").agg(crossings=pl.len())
    return crossing_splits, splitter


@app.cell
def split_cells(a5, crossing_splits, mo, pl):
    def _cell_of(lon, lat):
        return a5.lonlat_to_cell((lon, lat), 8)

    split_cells = (
        crossing_splits.with_columns(
            cell=pl.struct("lon", "lat").map_elements(
                lambda _point: _cell_of(_point["lon"], _point["lat"]),
                return_dtype=pl.UInt64,
            )
        )
        .group_by("cell")
        .agg(
            crossings=pl.len(),
            splits=pl.col("split").n_unique(),
            split=pl.col("split").first(),
        )
    )
    mo.vstack(
        [
            mo.md(
                f"{split_cells.height:,} cells hold a crossing. Each holds crossings of one split "
                f"only: {split_cells['splits'].max() == 1}."
            ),
            split_cells.group_by("split").agg(
                cells=pl.len(), crossings=pl.col("crossings").sum()
            ),
        ]
    )
    return (split_cells,)


@app.cell
def split_map(
    a5,
    crossing_splits,
    geometry_type,
    lonboard,
    mo,
    np,
    pl,
    shapely,
    split_cells,
):
    _split_colours = {"tune": [42, 111, 219], "test": [232, 133, 12]}
    _shapes = split_cells.with_columns(
        geometry=pl.Series(
            shapely.to_wkb(
                [
                    shapely.Polygon(
                        a5.cell_to_boundary(_cell, {"segments": 20})
                    )
                    for _cell in split_cells["cell"]
                ]
            )
        ).cast(geometry_type)
    )
    mo.vstack(
        [
            mo.md(
                "Each cell holding a crossing, filled blue for tune and orange for test, with its "
                "crossings as dots"
            ),
            lonboard.Map(
                [
                    lonboard.PolygonLayer(
                        table=_shapes.select(
                            "cell", "split", "crossings", "geometry"
                        ).to_arrow(),
                        get_fill_color=np.array(
                            [
                                _split_colours[_s] + [70]
                                for _s in _shapes["split"]
                            ],
                            dtype=np.uint8,
                        ),
                        get_line_color=[80, 80, 80, 120],
                        line_width_min_pixels=0.5,
                    ),
                    lonboard.ScatterplotLayer(
                        table=crossing_splits.select(
                            "crossing_id", "split", "geometry"
                        ).to_arrow(),
                        get_fill_color=np.array(
                            [
                                _split_colours[_s]
                                for _s in crossing_splits["split"]
                            ],
                            dtype=np.uint8,
                        ),
                        radius_min_pixels=1.5,
                    ),
                ],
                basemap=lonboard.basemap.MaplibreBasemap(
                    style=lonboard.basemap.CartoStyle.Positron
                ),
            ),
        ]
    )
    return


@app.cell
def scored_heading(mo):
    mo.md("""
    #### Counting one split, for one session

    A run replays each picked session in full, from its first sample to its last, so the predictor
    sees the whole journey. Scoring the tune split then counts only the passes of crossings in tune
    cells, and only the time the train spends in tune cells, as the session's scored duration. The
    session below is the one of those picked with the most passes in each split.
    """)
    return


@app.cell
def picked_passes(crossing_splits, lookout_medallion, mo, pa, picked, pl):
    _passes = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, crossing_id, crossed_at FROM train_session_crossing_v1"
            )
        )
    ).join(picked, on="session_id", how="semi")
    picked_passes = _passes.join(
        crossing_splits.select("crossing_id", "crossing_compact_id", "split"),
        on="crossing_id",
    )
    mo.vstack(
        [
            mo.md(
                f"{_passes.height:,} passes of the picked sessions, {picked_passes.height:,} of "
                "them at a crossing in the cover. Silver keeps the passes apart from the country's "
                "datasets, so the binding cannot join them in one query, and the join is here."
            ),
            picked_passes.group_by("split").agg(passes=pl.len()),
        ]
    )
    return (picked_passes,)


@app.cell
def scored_session(picked_passes, pl):
    scored_session = (
        picked_passes.group_by("session_id")
        .agg(
            tune=(pl.col("split") == "tune").sum(),
            test=(pl.col("split") == "test").sum(),
        )
        .with_columns(fewer=pl.min_horizontal("tune", "test"))
        .sort("fewer", "session_id", descending=[True, False])
        .row(0, named=True)
    )
    scored_session
    return (scored_session,)


@app.cell
def scored_samples(lookout_medallion, pa, pl, scored_session, splitter):
    _samples = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT t, lon, lat FROM train_session_sample_v3 "
                "WHERE session_id = $session_id ORDER BY t, seq",
                params={"session_id": scored_session["session_id"]},
                country="DE",
            )
        )
    )
    scored_samples = _samples.with_columns(
        split=pl.Series(
            splitter.splits(
                pa.array(_samples["lon"].to_numpy()),
                pa.array(_samples["lat"].to_numpy()),
            )
        ),
        until=pl.col("t").shift(-1),
    )
    scored_samples.group_by("split").agg(samples=pl.len())
    return (scored_samples,)


@app.cell
def scored_time(
    compared,
    lookout_medallion,
    mo,
    pa,
    pl,
    scored_samples,
    scored_session,
):
    _recorded = pl.from_arrow(
        pa.table(
            lookout_medallion.query_gold(
                "SELECT duration, scored_duration FROM evaluation_session_v2 "
                "WHERE run_id = $run_id AND session_id = $session_id",
                params={
                    "run_id": compared["crow_flies"],
                    "session_id": scored_session["session_id"],
                },
            )
        )
    )
    mo.vstack(
        [
            mo.md(
                "The time from each sample to the next, by the split of the sample's cell"
            ),
            scored_samples.group_by("split").agg(
                time=(pl.col("until") - pl.col("t")).sum()
            ),
            mo.md(
                "The duration and the scored duration the sweep recorded for the session"
            ),
            _recorded,
        ]
    )
    return


@app.cell
def scored_map(
    crossings,
    geometry_type,
    lonboard,
    mo,
    np,
    picked_passes,
    pl,
    scored_samples,
    scored_session,
    shapely,
):
    _split_colours = {"tune": [42, 111, 219], "test": [232, 133, 12]}
    _steps = (
        scored_samples.filter(pl.col("until").is_not_null())
        .with_columns(
            lon_next=pl.col("lon").shift(-1), lat_next=pl.col("lat").shift(-1)
        )
        .filter(pl.col("lon_next").is_not_null())
    )
    _segments = _steps.select(
        "split",
        geometry=pl.Series(
            shapely.to_wkb(
                shapely.linestrings(
                    np.stack(
                        [
                            _steps.select("lon", "lat").to_numpy(),
                            _steps.select("lon_next", "lat_next").to_numpy(),
                        ],
                        axis=1,
                    )
                )
            )
        ).cast(geometry_type),
    )
    _passed = picked_passes.filter(
        pl.col("session_id") == scored_session["session_id"]
    ).join(
        crossings.select("crossing_compact_id", "geometry"),
        on="crossing_compact_id",
    )
    mo.vstack(
        [
            mo.md(
                "The session's track, blue in tune cells and orange in test cells. Each crossing it "
                "passes is a dot in the same colours: the blue passes count on the tune split, and "
                "the orange ones wait for the test run."
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer(
                        table=_segments.to_arrow(),
                        get_color=np.array(
                            [_split_colours[_s] for _s in _segments["split"]],
                            dtype=np.uint8,
                        ),
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer(
                        table=_passed.select(
                            "crossing_id", "crossed_at", "split", "geometry"
                        ).to_arrow(),
                        get_fill_color=np.array(
                            [_split_colours[_s] for _s in _passed["split"]],
                            dtype=np.uint8,
                        ),
                        get_line_color=[255, 255, 255],
                        stroked=True,
                        line_width_min_pixels=1,
                        radius_min_pixels=5,
                    ),
                ],
                basemap=lonboard.basemap.MaplibreBasemap(
                    style=lonboard.basemap.CartoStyle.Positron
                ),
            ),
        ]
    )
    return


@app.cell
def bootstrap_heading(mo):
    mo.md("""
    ### Step 4: the bootstrap

    A run's measures depend on the sessions it happened to pick. The bootstrap estimates how far they
    would move under another draw of sessions like these. It draws the run's sessions again, at
    random and with replacement, as many as there are, and computes each measure on the draw. Across
    1,000 draws, the middle 95% is the measure's interval.

    The cells below repeat this for recall, in polars, over the sessions with time in tune cells. The
    run draws in Rust, with its own random numbers and its sessions without scored time too, so its
    interval differs slightly from these.
    """)
    return


@app.cell
def resampled(pl, session_measures):
    def _recalls(draw):
        return draw.select(
            light_cone=pl.col("credited_lc").sum() / pl.col("passes").sum(),
            crow_flies=pl.col("credited_cf").sum() / pl.col("passes").sum(),
        ).row(0, named=True)

    resampled = pl.DataFrame(
        [
            {
                "draw": _draw,
                **_recalls(
                    session_measures.sample(
                        fraction=1.0, with_replacement=True, seed=_draw
                    )
                ),
            }
            for _draw in range(1000)
        ]
    ).with_columns(difference=pl.col("light_cone") - pl.col("crow_flies"))
    resampled.head()
    return (resampled,)


@app.cell
def intervals(
    compared,
    lookout_medallion,
    mo,
    pa,
    pl,
    plt,
    predictor_colours,
    predictor_names,
    resampled,
):
    _summaries = pl.from_arrow(
        pa.table(
            lookout_medallion.query_gold(
                "SELECT run_id, recall_low, recall, recall_high FROM evaluation_summary_v2 "
                "WHERE run_id IN ($light_cone, $crow_flies)",
                params=compared,
            )
        )
    ).join(
        pl.DataFrame(
            {
                "run_id": list(compared.values()),
                "measure": list(compared.keys()),
            }
        ),
        on="run_id",
    )
    _drawn = (
        resampled.unpivot(index="draw", variable_name="measure")
        .group_by("measure")
        .agg(
            low=pl.col("value").quantile(0.025),
            high=pl.col("value").quantile(0.975),
        )
    )
    _fig, (_recall, _difference) = plt.subplots(1, 2, figsize=(10, 3.2))
    for _predictor, _colour in predictor_colours.items():
        _recall.hist(
            resampled[_predictor],
            bins=40,
            color=_colour,
            label=predictor_names[_predictor],
        )
    _recall.set_xlabel("recall in each of 1,000 draws")
    _recall.legend(frameon=False)
    _difference.hist(resampled["difference"], bins=40, color="#5b6770")
    _difference.set_xlabel("LightCone − CrowFlies, both on the same draw")
    for _ax in (_recall, _difference):
        _ax.set_ylabel("draws")
        _ax.spines[["top", "right"]].set_visible(False)
    mo.vstack(
        [
            _fig,
            mo.md(
                "The intervals from these draws, and those each run recorded"
            ),
            _drawn.join(
                _summaries.drop("run_id"), on="measure", how="left"
            ).sort("measure"),
            mo.md(
                "The difference's interval comes from computing both recalls on each draw, which "
                "pairs them. Each run's own interval comes from its draws alone."
            ),
        ]
    )
    return


@app.cell
def recap(
    compared_sessions,
    crossing_splits,
    filtered,
    mo,
    picked,
    pl,
    session_measures,
):
    mo.vstack(
        [
            mo.md("### What each step leaves"),
            pl.DataFrame(
                {
                    "step": [
                        "DE train sessions in silver",
                        "1. kept by the filter",
                        "2. picked by the sweep (the earlier phases picked 1,000)",
                        "3. with time in tune cells, so counted",
                        "compared session by session in this notebook",
                        "DE crossings",
                        "3. crossings in tune cells",
                    ],
                    "count": [
                        filtered["sessions"].item(),
                        filtered["kept"].item(),
                        picked.height,
                        session_measures.height,
                        compared_sessions.height,
                        crossing_splits.height,
                        crossing_splits.filter(
                            pl.col("split") == "tune"
                        ).height,
                    ],
                }
            ),
        ]
    )
    return


@app.cell
def _():
    import a5
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import numpy as np
    import polars as pl
    import pyarrow as pa
    import shapely

    import lookout_evaluation as evaluation
    import lookout_medallion

    return (
        a5,
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
                    "s.notifications, s.precision, s.recall, s.false_alarms_per_hour, s.repeats, "
                    "s.lead_median FROM evaluation_run_v6 r JOIN evaluation_summary_v2 s USING (run_id) "
                    "WHERE r.run_date = '2026-11-16' AND s.sessions > 1000"
                )
            )
        )
        .with_columns(
            setting=pl.coalesce(
                pl.col("predictor_settings").map.get("half_width_degrees"),
                pl.col("predictor_settings").map.get("radius_metres"),
            ).cast(pl.Int32)
        )
        .drop("predictor_settings")
        .sort("predictor", "setting", descending=[True, False])
    )
    sweep
    return (sweep,)


@app.cell
def light_cone_picker(mo, pl, sweep):
    light_cone_choice = mo.ui.dropdown(
        options={
            f"{_run['setting']}°": _run["run_id"]
            for _run in sweep.filter(
                pl.col("predictor") == "light_cone"
            ).iter_rows(named=True)
        },
        value="30°",
        label="LightCone's half-width",
    )
    light_cone_choice
    return (light_cone_choice,)


@app.cell
def compared_runs(light_cone_choice, pl, sweep):
    compared = {
        "light_cone": light_cone_choice.value,
        "crow_flies": sweep.filter(
            pl.col("predictor") == "crow_flies", pl.col("setting") == 1000
        )["run_id"].item(),
    }
    predictor_names = {
        "light_cone": "LightCone",
        "crow_flies": "CrowFlies 1 km",
    }
    predictor_colours = {"light_cone": "#2a6fdb", "crow_flies": "#e8850c"}
    compared
    return compared, predictor_colours, predictor_names


@app.cell
def outcome_counts(compared, lookout_medallion, pa, pl):
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


@app.cell
def early_heading(mo):
    mo.md("""
    ## LightCone alerts too early

    A pass alerted more than the horizon ahead is credited as too early, at a reduced score.
    LightCone predicts arrival from the straight line to a crossing. That line is shorter than the
    track, so LightCone alerts sooner than the horizon allows. The chart places each credited pass by
    its lead time. The dashed lines mark the last moment, at 30 s, and the horizon, at 5 minutes.
    """)
    return


@app.cell
def lead_times(mo, np, outcomes, pl, plt, predictor_colours, predictor_names):
    _credited = outcomes.filter(pl.col("outcome") == "credited")
    _bins = np.arange(0, 961, 30)
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
    mo.vstack(
        [
            _fig,
            _credited.pivot(
                on="predictor",
                index="class",
                values="score",
                aggregate_function="len",
            ).sort("class"),
        ]
    )
    return


@app.cell
def realerted(mo, outcomes, pl):
    _light_cone = outcomes.filter(pl.col("predictor") == "light_cone")
    _pass = ["session_id", "crossing_compact_id", "crossed_at"]
    _too_early = _light_cone.filter(
        pl.col("outcome") == "credited", pl.col("class") == "too_early"
    ).select(*_pass, "score")
    _realerted = _too_early.join(
        _light_cone.filter(
            pl.col("outcome") == "repeat", pl.col("class") == "hit"
        )
        .select(_pass)
        .unique(),
        on=_pass,
        how="semi",
    )
    _credited = _light_cone.filter(pl.col("outcome") == "credited")[
        "score"
    ].sum()
    _passes = _light_cone.filter(
        pl.col("outcome").is_in(["credited", "miss"])
    ).height
    mo.md(f"""
    ### Most early alerts come round again in time

    LightCone credits {_too_early.height:,} passes as too early. In {_realerted.height:,} of them,
    {_realerted.height / _too_early.height:.0%}, the crossing leaves the cone and enters it again,
    and the second alert falls within the horizon. The evaluation scores the first alert before each
    pass, so that second alert counts as a repeat. Had each of those passes scored as a hit, recall
    would be {(_credited + (1 - _realerted["score"]).sum()) / _passes:.3f} rather than
    {_credited / _passes:.3f}. That bounds what holding back the early alert could gain, since
    holding it back changes when the crossing enters the cone too.
    """)
    return


@app.cell
def measures_heading(mo):
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
def session_measures_cell(
    compared,
    lookout_medallion,
    mo,
    outcomes,
    pa,
    pl,
    shapely,
):
    _scored = pl.from_arrow(
        pa.table(
            lookout_medallion.query_gold(
                "SELECT session_id, scored_duration FROM evaluation_session_v2 WHERE run_id = $run_id",
                params={"run_id": compared["crow_flies"]},
            )
        )
    ).filter(pl.col("scored_duration") > pl.duration(seconds=0))
    _tracks = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, geometry_projected, started_at, ended_at FROM train_session_v3",
                country="DE",
            )
        )
    )
    _speeds = _tracks.select(
        "session_id",
        km=pl.Series(
            shapely.length(
                shapely.from_wkb(_tracks["geometry_projected"].to_list())
            )
            / 1000
        ),
        hours=(pl.col("ended_at") - pl.col("started_at")).dt.total_seconds()
        / 3600,
    ).select("session_id", mean_kmh=pl.col("km") / pl.col("hours"))
    _tallies = outcomes.group_by("predictor", "session_id").agg(
        credited=pl.col("score").filter(pl.col("outcome") == "credited").sum(),
        passes=pl.col("outcome")
        .is_in(["credited", "miss"])
        .sum()
        .cast(pl.Int64),
        false_alarms=(pl.col("outcome") == "false_alarm").sum().cast(pl.Int64),
        repeats=(pl.col("outcome") == "repeat").sum().cast(pl.Int64),
    )

    def _of(predictor, suffix):
        return (
            _tallies.filter(pl.col("predictor") == predictor)
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
        _scored.join(_of("light_cone", "lc"), on="session_id", how="left")
        .join(_of("crow_flies", "cf"), on="session_id", how="left")
        .fill_null(0)
        .join(_speeds, on="session_id", how="left")
        .with_columns(
            hours=pl.col("scored_duration").dt.total_seconds() / 3600,
            passes=pl.col("passes_cf"),
        )
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
    return compared_sessions, session_measures


@app.cell
def distribution(
    compared_sessions,
    np,
    plt,
    predictor_colours,
    predictor_names,
):
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
def gains(compared_sessions, mo, np, pl, plt):
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
def by_speed(
    compared_sessions,
    mo,
    pl,
    plt,
    predictor_colours,
    predictor_names,
):
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
def examples_heading(mo):
    mo.md("""
    ## Sessions picked from the spread

    Rank the compared sessions by one difference between the predictors, and pick the session at each
    of the worst, p10, p25, the median, p75, p90, and the best, for LightCone. Selecting a row draws
    that session below.
    """)
    return


@app.cell
def ranking_picker(mo):
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
def examples_table(compared_sessions, mo, pl, ranking):
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


@app.cell
def crossings_cell(lookout_medallion, mo, pa, pl, shapely):
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


@app.cell
def by_crossing_heading(mo):
    mo.md("""
    ## Where LightCone raises more false alarms, by crossing

    Each crossing with a false alarm under either predictor is coloured by LightCone's false alarms
    less CrowFlies', red where LightCone raises more and blue where it raises fewer, and sized by the
    size of that difference.
    """)
    return


@app.cell
def by_crossing(crossings, lonboard, mo, np, outcomes, pl, plt):
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
        .with_columns(more=pl.col("light_cone") - pl.col("crow_flies"))
        .join(crossings, on="crossing_compact_id")
        .sort("more")
    )
    _more = false_alarms_by_crossing["more"].to_numpy()
    _limit = np.quantile(np.abs(_more), 0.99)
    _map = lonboard.Map(
        lonboard.ScatterplotLayer(
            table=false_alarms_by_crossing.select(
                "crossing_id", "light_cone", "crow_flies", "more", "geometry"
            ).to_arrow(),
            get_fill_color=lonboard.colormap.apply_continuous_cmap(
                (np.clip(_more, -_limit, _limit) / _limit + 1) / 2,
                plt.get_cmap("RdBu_r"),
            ),
            get_radius=np.sqrt(np.abs(_more)) * 40,
            radius_min_pixels=1.5,
        ),
        basemap=lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        ),
    )
    _columns = [
        "crossing_id",
        "light_cone",
        "crow_flies",
        "more",
        "lon",
        "lat",
    ]
    mo.vstack(
        [
            _map,
            mo.md(
                "Crossings where LightCone raises the most false alarms beyond CrowFlies"
            ),
            false_alarms_by_crossing.select(_columns).tail(10).reverse(),
            mo.md("Crossings where LightCone raises the fewest"),
            false_alarms_by_crossing.select(_columns).head(10),
        ]
    )
    return


@app.cell
def session_heading(mo):
    mo.md("""
    ## One session

    The track is grey. Each crossing the session alerted on or passed is coloured by its best
    outcome under that predictor: a hit, too early, at the last moment, a miss, or a false alarm only.
    A line joins the train, at each alert, to the crossing it alerted on, in the same colours. The
    table under each map counts that predictor's outcomes in the session.
    """)
    return


@app.cell
def session(examples, lookout_medallion, mo, pa, pl):
    example = (
        examples.value.row(0, named=True)
        if examples.value.height
        else examples.data.row(3, named=True)
    )
    session_samples = pl.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT t, lon, lat FROM train_session_sample_v3 "
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
def session_maps(
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
        _lines = _alerts.select(
            "kind",
            _geometries(
                shapely.linestrings(
                    np.stack(
                        [
                            _alerts.select(
                                "train_lon", "train_lat"
                            ).to_numpy(),
                            _alerts.select("lon", "lat").to_numpy(),
                        ],
                        axis=1,
                    )
                )
            ),
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
        if _lines.height:
            _layers.insert(
                1,
                lonboard.PathLayer(
                    table=_lines.to_arrow(),
                    get_color=np.column_stack(
                        [
                            _colours(_lines["kind"]),
                            np.full(_lines.height, 120, dtype=np.uint8),
                        ]
                    ),
                    width_min_pixels=1,
                ),
            )
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
