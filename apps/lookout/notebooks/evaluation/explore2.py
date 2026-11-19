# /// script
# dependencies = [
#     "marimo",
#     "polars[pyarrow]==2.1.0",
#     "lookout-medallion",
#     "lonboard==0.17.0",
#     "geopandas==1.2.0",
#     "folium>=0.12",
#     "mapclassify==2.11.0",
#     "matplotlib==3.11.2",
#     "pandas==3.1.1",
#     "shapely==2.2.0",
# ]
# requires-python = ">=3.13"
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def imports():
    import marimo as mo
    import pyarrow as pa
    import pandas as pd
    import geopandas as gpd
    import shapely
    import matplotlib
    import matplotlib.pyplot
    import lonboard
    import lookout_medallion

    return gpd, lonboard, lookout_medallion, matplotlib, mo, pa, pd, shapely


@app.cell
def silver_table(lookout_medallion, pa):
    def silver_table(sql):
        _table = pa.table(lookout_medallion.query_silver(sql, country="DE"))
        return _table.drop_columns(
            [
                _name
                for _name in _table.column_names
                if _name == "geometry_projected"
            ]
        )

    silver_table(
        "SELECT COUNT(*) AS samples, COUNT(DISTINCT session_id) AS sessions FROM train_session_sample_v2"
    )
    return (silver_table,)


@app.cell
def labelled_basemap(lonboard):
    def labelled_basemap():
        return lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        )

    return (labelled_basemap,)


@app.cell
def samples_per_session(silver_table):
    samples_per_session = silver_table("""
    SELECT COUNT(*) AS sessions,
           MIN(sample_count) AS min,
           approx_percentile_cont(0.01) WITHIN GROUP (ORDER BY sample_count) AS p01,
           approx_percentile_cont(0.25) WITHIN GROUP (ORDER BY sample_count) AS p25,
           median(sample_count) AS p50,
           approx_percentile_cont(0.75) WITHIN GROUP (ORDER BY sample_count) AS p75,
           approx_percentile_cont(0.99) WITHIN GROUP (ORDER BY sample_count) AS p99,
           MAX(sample_count) AS max
    FROM train_session_v2
    """)
    samples_per_session
    return (samples_per_session,)


@app.cell
def low_tail_map(
    gpd,
    labelled_basemap,
    lonboard,
    samples_per_session,
    silver_table,
):
    low_tail_samples = gpd.GeoDataFrame.from_arrow(
        silver_table(f"""
    SELECT p.session_id, s.sample_count, p.seq, p.t, p.implied_speed_mps, p.geometry
    FROM train_session_sample_v2 p
    JOIN train_session_v2 s ON p.session_id = s.session_id
    WHERE s.sample_count <= {samples_per_session["p01"][0].as_py()}
    """)
    )
    lonboard.viz(low_tail_samples, map_kwargs={"basemap": labelled_basemap()})
    return


@app.cell
def high_tail_samples(gpd, samples_per_session, silver_table):
    high_tail_samples = gpd.GeoDataFrame.from_arrow(
        silver_table(f"""
    SELECT p.session_id, s.trip_id, s.source_id, COALESCE(s.source_label, '(no label)') AS source_label,
           s.sample_count, s.started_at, s.ended_at,
           p.seq, p.t, p.geometry_source, p.implied_speed_mps, p.lon, p.lat, p.geometry
    FROM train_session_sample_v2 p
    JOIN train_session_v2 s ON p.session_id = s.session_id
    WHERE s.sample_count >= {samples_per_session["p99"][0].as_py()}
    ORDER BY p.session_id, p.t, p.seq
    """)
    )
    high_tail_samples.groupby(["source_label", "geometry_source"]).agg(
        sessions=("session_id", "nunique"), samples=("session_id", "size")
    ).reset_index()
    return (high_tail_samples,)


@app.cell
def high_tail_lines_map(
    gpd,
    high_tail_samples,
    labelled_basemap,
    lonboard,
    pd,
    shapely,
):
    _codes, _session_ids = pd.factorize(
        high_tail_samples.session_id, sort=True
    )
    _sessions = high_tail_samples.groupby("session_id", sort=True)[
        [
            "trip_id",
            "source_id",
            "source_label",
            "sample_count",
            "started_at",
            "ended_at",
        ]
    ].first()
    high_tail_sessions = gpd.GeoDataFrame(
        _sessions.loc[_session_ids].reset_index(),
        geometry=shapely.linestrings(
            high_tail_samples.lon.to_numpy(),
            high_tail_samples.lat.to_numpy(),
            indices=_codes,
        ),
        crs="OGC:CRS84",
    )
    lonboard.viz(
        high_tail_sessions, map_kwargs={"basemap": labelled_basemap()}
    )
    return (high_tail_sessions,)


@app.cell
def high_tail_by_source_map(
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    matplotlib,
    mo,
):
    _counts = (
        high_tail_sessions.groupby(["source_id", "source_label"])
        .size()
        .rename("sessions")
        .sort_values(ascending=False)
        .reset_index()
    )
    _palette = matplotlib.colormaps["tab10"]
    _colours = {
        _source_id: tuple(int(255 * _channel) for _channel in _palette(_i)[:3])
        for _i, _source_id in enumerate(_counts.source_id)
    }
    _drawn = (
        high_tail_sessions.set_index("source_id")
        .loc[_counts.source_id]
        .reset_index()
    )
    mo.hstack(
        [
            lonboard.Map(
                lonboard.PathLayer.from_geopandas(
                    _drawn,
                    get_color=lonboard.colormap.apply_categorical_cmap(
                        _drawn.source_id, _colours
                    ),
                    width_min_pixels=2,
                ),
                basemap=labelled_basemap(),
            ),
            mo.md(
                "\n".join(
                    f'<span style="color: rgb{_colours[_row.source_id]}">■</span> '
                    f"{_row.source_label} `{_row.source_id[:8]}`: {_row.sessions}  "
                    for _row in _counts.itertuples()
                )
            ),
        ],
        widths=[4, 1],
    )
    return


@app.cell
def high_tail_stops_map(
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _stops = high_tail_samples[high_tail_samples.geometry_source == "stop"][
        ["session_id", "seq", "t", "implied_speed_mps", "geometry"]
    ]
    mo.vstack(
        [
            mo.md(
                f"{len(high_tail_sessions):,} sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        high_tail_sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_two_stops_map(
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _stop_counts = (
        high_tail_samples[high_tail_samples.geometry_source == "stop"]
        .groupby("session_id")
        .size()
    )
    _session_ids = _stop_counts[_stop_counts == 2].index
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} sessions with two stops as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_many_stops_map(
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _stop_counts = (
        high_tail_samples[high_tail_samples.geometry_source == "stop"]
        .groupby("session_id")
        .size()
    )
    _session_ids = _stop_counts[_stop_counts > 2].index
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} sessions with more than two stops as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_stops_per_session(high_tail_samples, matplotlib, mo):
    high_tail_stops_per_session = (
        high_tail_samples[high_tail_samples.geometry_source == "stop"]
        .groupby("session_id")
        .size()
        .value_counts()
        .sort_index()
        .rename_axis("stops")
        .rename("sessions")
        .reset_index()
    )
    _figure, _axes = matplotlib.pyplot.subplots(figsize=(9, 3.5))
    _axes.bar(
        high_tail_stops_per_session.stops,
        high_tail_stops_per_session.sessions,
        width=0.8,
        color="#2a6fdb",
    )
    _axes.set_xlabel("stops in the session")
    _axes.set_ylabel("sessions")
    _axes.set_title(
        f"Stops per high-tail session ({high_tail_stops_per_session.sessions.sum():,} sessions)"
    )
    _axes.spines[["top", "right"]].set_visible(False)
    _axes.grid(axis="y", color="#e5e5e5")
    _axes.set_axisbelow(True)
    mo.hstack([_figure, high_tail_stops_per_session], widths=[3, 1])
    return


@app.cell
def high_tail_few_stops_map(
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _stop_counts = (
        high_tail_samples[high_tail_samples.geometry_source == "stop"]
        .groupby("session_id")
        .size()
    )
    _session_ids = _stop_counts[_stop_counts <= 10].index
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} sessions with at most ten stops as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_more_stops_map(
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _stop_counts = (
        high_tail_samples[high_tail_samples.geometry_source == "stop"]
        .groupby("session_id")
        .size()
    )
    _session_ids = _stop_counts[_stop_counts > 10].index
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} sessions with more than ten stops as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def bad_sessions_state(mo):
    get_bad_sessions, set_bad_sessions = mo.state(frozenset())
    return get_bad_sessions, set_bad_sessions


@app.cell
def bad_sessions_picker(
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    pd,
    set_bad_sessions,
):
    _session_ids = high_tail_sessions.session_id.to_numpy()
    _picked = set()

    def _colours():
        _is_picked = high_tail_sessions.session_id.isin(_picked)
        return pd.DataFrame(
            {
                "r": _is_picked.map({True: 220, False: 120}),
                "g": _is_picked.map({True: 30, False: 120}),
                "b": _is_picked.map({True: 30, False: 120}),
            }
        ).to_numpy(dtype="uint8")

    bad_sessions_layer = lonboard.PathLayer.from_geopandas(
        high_tail_sessions,
        get_color=_colours(),
        width_min_pixels=2,
        pickable=True,
    )

    def _pick(_change):
        if _change.new is not None:
            _picked.add(_session_ids[_change.new])
            bad_sessions_layer.get_color = _colours()
            set_bad_sessions(lambda _bad: _bad | {_session_ids[_change.new]})

    bad_sessions_layer.observe(_pick, names="selected_index")
    lonboard.Map(bad_sessions_layer, basemap=labelled_basemap())
    return


@app.cell
def bad_sessions(get_bad_sessions, high_tail_sessions, mo):
    bad_sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(get_bad_sessions())
    ].drop(columns="geometry")
    mo.vstack([mo.md(f"{len(bad_sessions)} sessions picked"), bad_sessions])
    return


@app.cell
def max_speed_mps():
    MAX_SPEED_MPS = 100
    return (MAX_SPEED_MPS,)


@app.cell
def high_tail_checks(
    MAX_SPEED_MPS,
    get_bad_sessions,
    high_tail_samples,
    mo,
    pd,
):
    _by_session = high_tail_samples.groupby("session_id")
    high_tail_checks = (
        pd.DataFrame(
            {
                "max_speed_mps": _by_session.implied_speed_mps.max(),
                "stops": high_tail_samples[
                    high_tail_samples.geometry_source == "stop"
                ]
                .groupby("session_id")
                .size(),
            }
        )
        .fillna({"stops": 0})
        .rename_axis("session_id")
        .reset_index()
    )
    high_tail_checks["too_fast"] = (
        high_tail_checks.max_speed_mps > MAX_SPEED_MPS
    )
    high_tail_checks["two_stops"] = high_tail_checks.stops == 2
    high_tail_checks["bogus"] = (
        high_tail_checks.too_fast | high_tail_checks.two_stops
    )
    high_tail_checks["picked"] = high_tail_checks.session_id.isin(
        get_bad_sessions()
    )
    mo.vstack(
        [
            high_tail_checks.groupby(["too_fast", "two_stops"])
            .size()
            .rename("sessions")
            .reset_index(),
            high_tail_checks.groupby(["bogus", "picked"])
            .size()
            .rename("sessions")
            .reset_index(),
        ]
    )
    return (high_tail_checks,)


@app.cell
def high_tail_bogus_map(
    high_tail_checks,
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = high_tail_checks[high_tail_checks.bogus].session_id
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} bogus sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_ok_map(
    high_tail_checks,
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = high_tail_checks[~high_tail_checks.bogus].session_id
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} ok sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_speed_checks(high_tail_checks):
    high_tail_speed_checks = high_tail_checks[
        ["session_id", "max_speed_mps", "stops", "too_fast", "picked"]
    ].assign(bogus=high_tail_checks.too_fast)
    high_tail_speed_checks.groupby(["bogus", "picked"]).size().rename(
        "sessions"
    ).reset_index()
    return (high_tail_speed_checks,)


@app.cell
def high_tail_speed_bogus_map(
    high_tail_samples,
    high_tail_sessions,
    high_tail_speed_checks,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = high_tail_speed_checks[
        high_tail_speed_checks.bogus
    ].session_id
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} bogus sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_speed_ok_map(
    high_tail_samples,
    high_tail_sessions,
    high_tail_speed_checks,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = high_tail_speed_checks[
        ~high_tail_speed_checks.bogus
    ].session_id
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} ok sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def germany(lookout_medallion, pa, shapely):
    _areas = pa.table(
        lookout_medallion.query_bronze(
            "SELECT class, extract_id, geometry FROM area WHERE division_id = $division AND subtype = 'country'",
            tables={"area": {"theme": "divisions", "type": "division_area"}},
            params={"division": lookout_medallion.division_id("DE")},
        )
    ).to_pandas()
    _areas = _areas[_areas.extract_id == _areas.extract_id.max()]
    germany = shapely.union_all(shapely.from_wkb(_areas.geometry))
    shapely.prepare(germany)
    _areas.drop(columns="geometry")
    return (germany,)


@app.cell
def high_tail_germany_checks(
    germany,
    high_tail_samples,
    high_tail_speed_checks,
    shapely,
):
    _outside = (
        high_tail_samples.assign(
            outside=~shapely.contains_xy(
                germany,
                high_tail_samples.lon.to_numpy(),
                high_tail_samples.lat.to_numpy(),
            )
        )
        .groupby("session_id")
        .outside.sum()
        .rename("samples_outside_germany")
    )
    high_tail_germany_checks = high_tail_speed_checks.merge(
        _outside, left_on="session_id", right_index=True, validate="one_to_one"
    )
    high_tail_germany_checks["outside_germany"] = (
        high_tail_germany_checks.samples_outside_germany > 0
    )
    high_tail_germany_checks["bogus"] = (
        high_tail_germany_checks.too_fast
        | high_tail_germany_checks.outside_germany
    )
    high_tail_germany_checks.groupby(
        ["too_fast", "outside_germany", "picked"]
    ).size().rename("sessions").reset_index()
    return (high_tail_germany_checks,)


@app.cell
def high_tail_germany_bogus_map(
    high_tail_germany_checks,
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = high_tail_germany_checks[
        high_tail_germany_checks.bogus
    ].session_id
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} bogus sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def high_tail_germany_ok_map(
    high_tail_germany_checks,
    high_tail_samples,
    high_tail_sessions,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = high_tail_germany_checks[
        ~high_tail_germany_checks.bogus
    ].session_id
    _sessions = high_tail_sessions[
        high_tail_sessions.session_id.isin(_session_ids)
    ]
    _stops = high_tail_samples[
        (high_tail_samples.geometry_source == "stop")
        & high_tail_samples.session_id.isin(_session_ids)
    ][["session_id", "seq", "t", "implied_speed_mps", "geometry"]]
    mo.vstack(
        [
            mo.md(
                f"{len(_sessions):,} ok sessions as paths; "
                f"{len(_stops):,} stops over {_stops.session_id.nunique():,} sessions"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _sessions,
                        get_color=[120, 120, 120],
                        width_min_pixels=2,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _stops,
                        get_fill_color=[220, 30, 30],
                        radius_min_pixels=3,
                        radius_max_pixels=6,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def all_session_checks(
    MAX_SPEED_MPS,
    germany,
    lookout_medallion,
    mo,
    pa,
    shapely,
    silver_table,
):
    def _samples_outside_germany_on(date):
        _samples = pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, lon, lat FROM train_session_sample_v2 WHERE sample_date = $date",
                country="DE",
                params={"date": date},
            )
        )
        _outside = ~shapely.contains_xy(
            germany, _samples["lon"].to_numpy(), _samples["lat"].to_numpy()
        )
        return (
            pa.table(
                {
                    "session_id": _samples["session_id"],
                    "outside": pa.array(_outside),
                }
            )
            .group_by("session_id")
            .aggregate([("outside", "sum")])
        )

    _dates = silver_table(
        "SELECT DISTINCT sample_date FROM train_session_sample_v2"
    )["sample_date"].to_pylist()
    _outside = (
        pa.concat_tables(
            [_samples_outside_germany_on(_date) for _date in sorted(_dates)]
        )
        .group_by("session_id")
        .aggregate([("outside_sum", "sum")])
        .rename_columns(["session_id", "samples_outside_germany"])
        .to_pandas()
    )
    _sessions = silver_table("""
    SELECT s.session_id, s.trip_id, s.source_id, COALESCE(s.source_label, '(no label)') AS source_label,
           s.sample_count, s.started_at, s.ended_at, m.max_speed_mps
    FROM train_session_v2 s
    JOIN (
        SELECT session_id, MAX(implied_speed_mps) AS max_speed_mps
        FROM train_session_sample_v2
        GROUP BY session_id
    ) m ON s.session_id = m.session_id
    """).to_pandas()
    all_session_checks = _sessions.merge(
        _outside, on="session_id", how="left", validate="one_to_one"
    )
    all_session_checks["too_fast"] = (
        all_session_checks.max_speed_mps > MAX_SPEED_MPS
    )
    all_session_checks["outside_germany"] = (
        all_session_checks.samples_outside_germany > 0
    )
    all_session_checks["excluded"] = (
        all_session_checks.too_fast | all_session_checks.outside_germany
    )
    mo.vstack(
        [
            all_session_checks.groupby("excluded")
            .agg(
                sessions=("session_id", "size"),
                samples=("sample_count", "sum"),
            )
            .reset_index(),
            all_session_checks.groupby(["too_fast", "outside_germany"])
            .agg(
                sessions=("session_id", "size"),
                samples=("sample_count", "sum"),
            )
            .reset_index(),
        ]
    )
    return (all_session_checks,)


@app.cell
def all_session_paths(
    gpd,
    lookout_medallion,
    mo,
    pa,
    pd,
    shapely,
    silver_table,
):
    PATH_TOLERANCE_DEGREES = 0.002

    def _paths_on(date):
        _samples = pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, lon, lat FROM train_session_sample_v2 WHERE sample_date = $date ORDER BY session_id, t, seq",
                country="DE",
                params={"date": date},
            )
        ).to_pandas()
        _drawable = _samples[
            _samples.groupby("session_id").session_id.transform("size") >= 2
        ]
        _codes, _session_ids = pd.factorize(_drawable.session_id, sort=True)
        return gpd.GeoDataFrame(
            {"session_id": _session_ids, "sample_date": date},
            geometry=shapely.simplify(
                shapely.linestrings(
                    _drawable.lon.to_numpy(),
                    _drawable.lat.to_numpy(),
                    indices=_codes,
                ),
                PATH_TOLERANCE_DEGREES,
            ),
            crs="OGC:CRS84",
        )

    _dates = sorted(
        silver_table(
            "SELECT DISTINCT sample_date FROM train_session_sample_v2"
        )["sample_date"].to_pylist()
    )
    all_session_paths = pd.concat(
        [_paths_on(_date) for _date in _dates], ignore_index=True
    )
    mo.md(
        f"{len(all_session_paths):,} paths, one per session and day, over "
        f"{all_session_paths.session_id.nunique():,} sessions; "
        f"{int(shapely.get_num_coordinates(all_session_paths.geometry).sum()):,} vertices "
        f"after simplifying to {PATH_TOLERANCE_DEGREES}°"
    )
    return (all_session_paths,)


@app.cell
def all_sessions_excluded_map(
    all_session_checks,
    all_session_paths,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = all_session_checks[all_session_checks.excluded].session_id
    _paths = all_session_paths[
        all_session_paths.session_id.isin(_session_ids)
    ].merge(
        all_session_checks[
            [
                "session_id",
                "trip_id",
                "source_label",
                "max_speed_mps",
                "samples_outside_germany",
            ]
        ],
        on="session_id",
        validate="many_to_one",
    )
    mo.vstack(
        [
            mo.md(
                f"{_paths.session_id.nunique():,} of {len(_session_ids):,} excluded sessions drawn, "
                f"as {len(_paths):,} paths"
            ),
            lonboard.Map(
                lonboard.PathLayer.from_geopandas(
                    _paths, get_color=[120, 120, 120], width_min_pixels=2
                ),
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def all_sessions_kept_map(
    all_session_checks,
    all_session_paths,
    labelled_basemap,
    lonboard,
    mo,
):
    _session_ids = all_session_checks[~all_session_checks.excluded].session_id
    _paths = all_session_paths[
        all_session_paths.session_id.isin(_session_ids)
    ].merge(
        all_session_checks[
            [
                "session_id",
                "trip_id",
                "source_label",
                "max_speed_mps",
                "samples_outside_germany",
            ]
        ],
        on="session_id",
        validate="many_to_one",
    )
    mo.vstack(
        [
            mo.md(
                f"{_paths.session_id.nunique():,} of {len(_session_ids):,} kept sessions drawn, "
                f"as {len(_paths):,} paths"
            ),
            lonboard.Map(
                lonboard.PathLayer.from_geopandas(
                    _paths, get_color=[120, 120, 120], width_min_pixels=2
                ),
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


@app.cell
def v3_counts(all_session_checks, mo, pd, silver_table):
    v3_sessions = silver_table("""
    SELECT session_id, trip_id, source_id, COALESCE(source_label, '(no label)') AS source_label,
           sample_count, realtime_percentage, started_at, ended_at
    FROM train_session_v3
    """).to_pandas()
    _v3_samples = silver_table(
        "SELECT COUNT(*) AS samples FROM train_session_sample_v3"
    )["samples"][0].as_py()
    _kept = all_session_checks[~all_session_checks.excluded]
    _kept_ids = set(_kept.session_id)
    _v3_ids = set(v3_sessions.session_id)
    mo.vstack(
        [
            pd.DataFrame(
                {
                    "measure": ["sessions", "samples"],
                    "kept by all_session_checks, from v2": [
                        len(_kept),
                        int(_kept.sample_count.sum()),
                    ],
                    "written to train_session_v3": [
                        len(v3_sessions),
                        _v3_samples,
                    ],
                }
            ),
            pd.DataFrame(
                {
                    "measure": [
                        "kept by all_session_checks, missing from v3",
                        "in v3, excluded by all_session_checks",
                    ],
                    "sessions": [
                        len(_kept_ids - _v3_ids),
                        len(_v3_ids - _kept_ids),
                    ],
                }
            ),
        ]
    )
    return (v3_sessions,)


@app.cell
def v3_realtime_percentage(matplotlib, v3_sessions):
    _at_least_90 = int((v3_sessions.realtime_percentage >= 90).sum())
    _figure, _axes = matplotlib.pyplot.subplots(figsize=(9, 3.5))
    _axes.hist(
        v3_sessions.realtime_percentage,
        bins=range(0, 105, 5),
        color="#2a6fdb",
        rwidth=0.9,
    )
    _axes.set_xlabel("realtime_percentage")
    _axes.set_ylabel("sessions")
    _axes.set_title(
        f"{_at_least_90:,} of {len(v3_sessions):,} sessions in train_session_v3 have realtime_percentage ≥ 90"
    )
    _axes.spines[["top", "right"]].set_visible(False)
    _axes.grid(axis="y", color="#e5e5e5")
    _axes.set_axisbelow(True)
    _figure
    return


@app.cell
def v3_session_paths(
    gpd,
    lookout_medallion,
    mo,
    pa,
    pd,
    shapely,
    silver_table,
):
    _tolerance_degrees = 0.002

    def _v3_paths_on(date):
        _samples = pa.table(
            lookout_medallion.query_silver(
                "SELECT session_id, lon, lat FROM train_session_sample_v3 WHERE sample_date = $date ORDER BY session_id, t, seq",
                country="DE",
                params={"date": date},
            )
        ).to_pandas()
        _drawable = _samples[
            _samples.groupby("session_id").session_id.transform("size") >= 2
        ]
        _codes, _session_ids = pd.factorize(_drawable.session_id, sort=True)
        return gpd.GeoDataFrame(
            {"session_id": _session_ids, "sample_date": date},
            geometry=shapely.simplify(
                shapely.linestrings(
                    _drawable.lon.to_numpy(),
                    _drawable.lat.to_numpy(),
                    indices=_codes,
                ),
                _tolerance_degrees,
            ),
            crs="OGC:CRS84",
        )

    _dates = sorted(
        silver_table(
            "SELECT DISTINCT sample_date FROM train_session_sample_v3"
        )["sample_date"].to_pylist()
    )
    v3_session_paths = pd.concat(
        [_v3_paths_on(_date) for _date in _dates], ignore_index=True
    )
    mo.md(
        f"{len(v3_session_paths):,} paths, one per session and day, over "
        f"{v3_session_paths.session_id.nunique():,} sessions; "
        f"{int(shapely.get_num_coordinates(v3_session_paths.geometry).sum()):,} vertices "
        f"after simplifying to {_tolerance_degrees}°"
    )
    return (v3_session_paths,)


@app.cell
def v3_sessions_map(
    labelled_basemap,
    lonboard,
    mo,
    v3_session_paths,
    v3_sessions,
):
    _paths = v3_session_paths.merge(
        v3_sessions[
            ["session_id", "trip_id", "source_label", "realtime_percentage"]
        ],
        on="session_id",
        validate="many_to_one",
    )
    _realtime = _paths.realtime_percentage >= 90
    mo.vstack(
        [
            mo.md(
                f"{_paths.session_id.nunique():,} sessions of train_session_v3 drawn as {len(_paths):,} paths: "
                f"blue where realtime_percentage ≥ 90 ({_paths[_realtime].session_id.nunique():,} sessions), "
                f"grey elsewhere"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _paths[~_realtime],
                        get_color=[150, 150, 150],
                        width_min_pixels=1,
                    ),
                    lonboard.PathLayer.from_geopandas(
                        _paths[_realtime],
                        get_color=[42, 111, 219],
                        width_min_pixels=2,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
        ]
    )
    return


if __name__ == "__main__":
    app.run()
