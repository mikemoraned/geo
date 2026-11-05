# /// script
# dependencies = [
#     "marimo",
#     "polars[pyarrow]==2.1.0",
#     "lookout-medallion",
#     "lonboard==0.17.0",
#     "geopandas==1.2.0",
#     "matplotlib==3.11.2",
#     "pandas==3.1.1",
#     "shapely==2.2.0",
#     "numpy==2.5.4",
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
    import numpy as np
    import pyarrow as pa
    import pyarrow.compute as pc
    import pandas as pd
    import geopandas as gpd
    import shapely
    import matplotlib.pyplot as plt
    import lonboard
    import lookout_medallion

    return gpd, lonboard, lookout_medallion, mo, np, pa, pc, pd, plt, shapely


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
        "SELECT COUNT(*) AS sessions, "
        "SUM(CASE WHEN realtime_percentage >= 90 THEN 1 ELSE 0 END) AS realtime_percentage_at_least_90 "
        "FROM train_session_v3"
    )
    return (silver_table,)


@app.cell
def kept_sessions(mo, silver_table):
    REALTIME_PERCENTAGE_KEPT = 90

    kept_sessions = silver_table(
        "SELECT session_id, sample_count, realtime_percentage FROM train_session_v3 "
        f"WHERE realtime_percentage >= {REALTIME_PERCENTAGE_KEPT}"
    )
    mo.vstack(
        [
            mo.md(
                f"{kept_sessions.num_rows:,} sessions with realtime_percentage ≥ {REALTIME_PERCENTAGE_KEPT}"
            ),
            kept_sessions.column("sample_count")
            .to_pandas()
            .describe(percentiles=[0.01, 0.5, 0.99])
            .round(1)
            .to_frame(),
        ]
    )
    return REALTIME_PERCENTAGE_KEPT, kept_sessions


@app.cell
def kept_samples(REALTIME_PERCENTAGE_KEPT, mo, silver_table):
    kept_samples_sql = f"""
        SELECT p.session_id, p.t,
               ST_X(p.geometry_projected) AS x, ST_Y(p.geometry_projected) AS y,
               p.lon, p.lat
        FROM train_session_sample_v3 p
        JOIN train_session_v3 s ON p.session_id = s.session_id
        WHERE s.realtime_percentage >= {REALTIME_PERCENTAGE_KEPT}
    """

    mo.vstack(
        [
            silver_table(
                "SELECT COUNT(*) AS samples, COUNT(DISTINCT session_id) AS sessions "
                f"FROM ({kept_samples_sql})"
            ),
            silver_table(
                f"SELECT * FROM ({kept_samples_sql}) ORDER BY session_id, t LIMIT 5"
            ),
        ]
    )
    return (kept_samples_sql,)


@app.cell
def steps(kept_samples_sql, silver_table):
    _in_session_order = "OVER (PARTITION BY session_id ORDER BY t, x, y)"
    steps_sql = f"""
        SELECT * FROM (
            SELECT session_id, t, x, y, lon, lat,
                   SQRT(
                       POWER(x - LAG(x) {_in_session_order}, 2)
                       + POWER(y - LAG(y) {_in_session_order}, 2)
                   ) AS step_m,
                   (CAST(t AS BIGINT) - CAST(LAG(t) {_in_session_order} AS BIGINT)) / 1000.0 AS step_s
            FROM ({kept_samples_sql})
        )
        WHERE step_m IS NOT NULL
    """

    silver_table(
        f"""SELECT COUNT(*) AS steps,
                   percentile_cont(step_m, 0.01) AS step_m_p1,
                   percentile_cont(step_m, 0.5) AS step_m_p50,
                   percentile_cont(step_m, 0.99) AS step_m_p99,
                   MAX(step_m) AS step_m_max,
                   percentile_cont(step_s, 0.01) AS step_s_p1,
                   percentile_cont(step_s, 0.5) AS step_s_p50,
                   percentile_cont(step_s, 0.99) AS step_s_p99,
                   MAX(step_s) AS step_s_max
            FROM ({steps_sql})"""
    )
    return (steps_sql,)


@app.cell
def spacing(lookout_medallion, mo, pa, silver_table, steps_sql):
    (MATCH_RADIUS_M,) = pa.table(
        lookout_medallion.query_silver(
            "SELECT DISTINCT match_radius_m FROM train_session_crossing_v1"
        )
    )["match_radius_m"].to_pylist()

    spacing = silver_table(
        f"""SELECT COUNT(*) AS steps,
                   AVG(CASE WHEN step_m > {MATCH_RADIUS_M} THEN 1.0 ELSE 0.0 END) AS share_over_radius,
                   AVG(CASE WHEN step_m > 2 * {MATCH_RADIUS_M} THEN 1.0 ELSE 0.0 END) AS share_over_twice_radius,
                   SUM(GREATEST(step_m - 2 * {MATCH_RADIUS_M}, 0)) / SUM(step_m) AS share_of_distance_beyond_radius,
                   COUNT(DISTINCT CASE WHEN step_m > 2 * {MATCH_RADIUS_M} THEN session_id END)
                       AS sessions_with_a_step_over_twice_radius
            FROM ({steps_sql})"""
    )
    _row = spacing.to_pylist()[0]
    mo.vstack(
        [
            mo.md(
                f"Against a match radius of {MATCH_RADIUS_M:g} m: "
                f"{_row['share_over_radius']:.2%} of steps are longer than the radius, and "
                f"{_row['share_over_twice_radius']:.3%} longer than twice it, in "
                f"{_row['sessions_with_a_step_over_twice_radius']:,} sessions. "
                f"{_row['share_of_distance_beyond_radius']:.4%} of the distance travelled lies "
                "more than the radius from every sample."
            ),
            spacing,
        ]
    )
    return (MATCH_RADIUS_M,)


@app.cell
def step_histogram(MATCH_RADIUS_M, mo, plt, silver_table, steps_sql):
    _BIN_M = 25
    step_bins = silver_table(
        f"""SELECT FLOOR(step_m / {_BIN_M}) * {_BIN_M} AS from_m, COUNT(*) AS steps
            FROM ({steps_sql}) GROUP BY 1 ORDER BY 1"""
    )

    _bins = step_bins.to_pandas()
    _figure, _axes = plt.subplots(figsize=(9, 4), facecolor="#fcfcfb")
    _axes.set_facecolor("#fcfcfb")
    _axes.bar(
        _bins.from_m,
        _bins.steps,
        width=_BIN_M,
        align="edge",
        color="#2a78d6",
        edgecolor="#fcfcfb",
        linewidth=1,
    )
    _axes.set_yscale("log")
    for _metres, _label in [
        (MATCH_RADIUS_M, "match radius"),
        (2 * MATCH_RADIUS_M, "twice the radius"),
    ]:
        _axes.axvline(_metres, color="#52514e", linewidth=1, linestyle="--")
        _axes.annotate(
            f"{_label}, {_metres:g} m",
            xy=(_metres, 1),
            xycoords=("data", "axes fraction"),
            xytext=(4, -4),
            textcoords="offset points",
            va="top",
            fontsize=9,
            color="#0b0b0b",
        )
    _axes.set_title(
        "Steps between consecutive samples of the kept sessions",
        loc="left",
        color="#0b0b0b",
    )
    _axes.set_xlabel("step (m)", color="#52514e")
    _axes.set_ylabel("steps (log scale)", color="#52514e")
    _axes.grid(axis="y", color="#e1e0d9", linewidth=0.5)
    _axes.set_axisbelow(True)
    for _side in ["top", "right"]:
        _axes.spines[_side].set_visible(False)
    for _side in ["left", "bottom"]:
        _axes.spines[_side].set_color("#c3c2b7")
    _axes.tick_params(colors="#52514e")

    mo.vstack(
        [
            mo.as_html(_figure),
            mo.accordion({"Steps in each 25 m bin": step_bins}),
        ]
    )
    return


@app.cell
def kept_paths(
    gpd,
    kept_samples_sql,
    lookout_medallion,
    mo,
    np,
    pa,
    shapely,
    silver_table,
):
    _samples = silver_table(
        f"SELECT session_id, x, y FROM ({kept_samples_sql}) ORDER BY session_id, t, x, y"
    )
    _sessions = (
        _samples.column("session_id")
        .combine_chunks()
        .cast(pa.string())
        .dictionary_encode()
    )
    _codes = _sessions.indices.to_numpy()
    _drawable = np.bincount(_codes)[_codes] >= 2
    _drawn_codes, _path_index = np.unique(
        _codes[_drawable], return_inverse=True
    )

    kept_paths = gpd.GeoDataFrame(
        {
            "session_id": _sessions.dictionary.take(
                pa.array(_drawn_codes)
            ).to_pylist()
        },
        geometry=shapely.linestrings(
            _samples.column("x").to_numpy()[_drawable],
            _samples.column("y").to_numpy()[_drawable],
            indices=_path_index,
        ),
        crs=lookout_medallion.projected_crs("DE"),
    )

    mo.vstack(
        [
            mo.md(
                f"{len(kept_paths):,} paths with {int(shapely.get_num_coordinates(kept_paths.geometry).sum()):,} vertices, "
                f"from {_samples.num_rows:,} samples of {len(_sessions.dictionary):,} sessions. "
                f"{len(_sessions.dictionary) - len(kept_paths):,} sessions hold a single sample, so draw no line."
            ),
            (kept_paths.length / 1000)
            .describe(percentiles=[0.01, 0.5, 0.99])
            .round(1)
            .rename("length_km")
            .to_frame(),
        ]
    )
    return (kept_paths,)


@app.cell
def near_crossings(
    MATCH_RADIUS_M,
    kept_paths,
    mo,
    np,
    pa,
    pc,
    shapely,
    silver_table,
):
    crossings = silver_table(
        """SELECT crossing_id,
                  ST_X(geometry_projected) AS x, ST_Y(geometry_projected) AS y,
                  ST_X(geometry) AS lon, ST_Y(geometry) AS lat
           FROM water_crossing_v2"""
    )
    _tree = shapely.STRtree(
        shapely.points(
            crossings.column("x").to_numpy(), crossings.column("y").to_numpy()
        )
    )
    _path_index, _crossing_index = _tree.query(
        np.asarray(kept_paths.geometry),
        predicate="dwithin",
        distance=MATCH_RADIUS_M,
    )
    near_crossings = pa.table(
        {
            "session_id": pa.array(
                kept_paths.session_id.to_numpy()[_path_index]
            ),
            "crossing_id": crossings.column("crossing_id")
            .cast(pa.string())
            .take(pa.array(_crossing_index)),
        }
    )

    mo.md(
        f"{near_crossings.num_rows:,} pairs of a path and a crossing within {MATCH_RADIUS_M:g} m of it, "
        f"over {len(pc.unique(near_crossings['session_id'])):,} of the {len(kept_paths):,} paths "
        f"and {len(pc.unique(near_crossings['crossing_id'])):,} of the {crossings.num_rows:,} crossings"
    )
    return crossings, near_crossings


@app.cell
def kept_passes(kept_sessions, lookout_medallion, mo, pa, pc):
    _passes = pa.table(
        lookout_medallion.query_silver(
            "SELECT session_id, crossing_id, crossed_at, distance_m, samples_within "
            "FROM train_session_crossing_v1"
        )
    )
    _passes = _passes.cast(
        pa.schema(
            [
                pa.field(_field.name, pa.string(), _field.nullable)
                if _field.name in ("session_id", "crossing_id")
                else _field
                for _field in _passes.schema
            ]
        )
    )
    kept_passes = _passes.filter(
        pc.is_in(
            _passes["session_id"],
            value_set=kept_sessions["session_id"]
            .combine_chunks()
            .cast(pa.string()),
        )
    )
    passed_pairs = kept_passes.group_by(
        ["session_id", "crossing_id"]
    ).aggregate([])

    mo.md(
        f"{kept_passes.num_rows:,} of the {_passes.num_rows:,} passes belong to kept sessions: "
        f"{passed_pairs.num_rows:,} pairs of a session and a crossing, over "
        f"{len(pc.unique(kept_passes['session_id'])):,} sessions"
    )
    return (passed_pairs,)


@app.cell
def missed(
    MATCH_RADIUS_M,
    crossings,
    kept_paths,
    mo,
    near_crossings,
    np,
    pa,
    passed_pairs,
    pd,
    shapely,
):
    _keys = ["session_id", "crossing_id"]
    _passed_off_every_path = passed_pairs.join(
        near_crossings, keys=_keys, join_type="left anti"
    )
    missed = (
        near_crossings.join(passed_pairs, keys=_keys, join_type="left anti")
        .to_pandas()
        .merge(
            crossings.cast(
                pa.schema(
                    [
                        pa.field(_field.name, pa.string())
                        if _field.name == "crossing_id"
                        else _field
                        for _field in crossings.schema
                    ]
                )
            ).to_pandas(),
            on="crossing_id",
            validate="many_to_one",
        )
        .sort_values(_keys, ignore_index=True)
    )
    _paths = (
        kept_paths.set_index("session_id")
        .geometry.loc[missed.session_id]
        .to_numpy()
    )
    _crossings = shapely.points(missed.x.to_numpy(), missed.y.to_numpy())
    _vertices, _vertex_path = shapely.get_coordinates(
        _paths, return_index=True
    )

    def _step_past(path, crossing):
        _coords = shapely.get_coordinates(path)
        _ends = np.cumsum(np.hypot(*np.diff(_coords, axis=0).T))
        _along = shapely.line_locate_point(path, crossing)
        _segment = min(int(np.searchsorted(_ends, _along)), len(_ends) - 1)
        return _ends[_segment] - (_ends[_segment - 1] if _segment else 0.0)

    missed["path_m"] = shapely.distance(_paths, _crossings)
    missed["nearest_sample_m"] = shapely.distance(
        shapely.multipoints(_vertices, indices=_vertex_path), _crossings
    )
    missed["step_m"] = [
        _step_past(_path, _crossing)
        for _path, _crossing in zip(_paths, _crossings)
    ]

    mo.vstack(
        [
            mo.md(
                f"{len(missed):,} of the {near_crossings.num_rows:,} pairs a path comes within "
                f"{MATCH_RADIUS_M:g} m of record no pass ({len(missed) / near_crossings.num_rows:.2%}), "
                f"over {missed.session_id.nunique():,} sessions and {missed.crossing_id.nunique():,} crossings. "
                f"{(missed.step_m > 2 * MATCH_RADIUS_M).sum():,} of them lie beside a step longer than twice the radius. "
                f"{_passed_off_every_path.num_rows:,} recorded pairs lie farther than the radius from their path."
            ),
            missed.groupby(
                pd.cut(
                    missed.path_m,
                    [0, 10, 25, 50, 100, 150, 200, MATCH_RADIUS_M],
                    include_lowest=True,
                ),
                observed=True,
            )
            .agg(
                misses=("path_m", "size"),
                beside_a_step_over_twice_radius=(
                    "step_m",
                    lambda _steps: int((_steps > 2 * MATCH_RADIUS_M).sum()),
                ),
                median_step_m=("step_m", "median"),
            )
            .round(1)
            .rename_axis("path_m"),
            missed[["path_m", "nearest_sample_m", "step_m"]]
            .describe(percentiles=[0.01, 0.5, 0.99])
            .round(1),
        ]
    )
    return (missed,)


@app.cell
def miss_map(gpd, kept_paths, lonboard, shapely):
    def labelled_basemap():
        return lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        )

    def miss_map(rows):
        _paths = kept_paths[
            kept_paths.session_id.isin(rows.session_id)
        ].to_crs("OGC:CRS84")
        _samples = gpd.GeoDataFrame(
            geometry=shapely.points(shapely.get_coordinates(_paths.geometry)),
            crs="OGC:CRS84",
        )
        _crossings = gpd.GeoDataFrame(
            rows[["crossing_id", "path_m", "nearest_sample_m", "step_m"]],
            geometry=gpd.points_from_xy(rows.lon, rows.lat),
            crs="OGC:CRS84",
        )
        return lonboard.Map(
            [
                lonboard.PathLayer.from_geopandas(
                    _paths, get_color=[42, 120, 214], width_min_pixels=2
                ),
                lonboard.ScatterplotLayer.from_geopandas(
                    _samples,
                    get_fill_color=[137, 135, 129],
                    radius_min_pixels=3,
                ),
                lonboard.ScatterplotLayer.from_geopandas(
                    _crossings,
                    get_fill_color=[235, 104, 52],
                    radius_min_pixels=6,
                ),
            ],
            basemap=labelled_basemap(),
            view_state={
                "longitude": rows.lon.mean(),
                "latitude": rows.lat.mean(),
                "zoom": 14,
            },
        )

    return labelled_basemap, miss_map


@app.cell
def missed_overview(
    gpd,
    kept_paths,
    labelled_basemap,
    lonboard,
    miss_map,
    missed,
    mo,
):
    _paths = kept_paths[kept_paths.session_id.isin(missed.session_id)].to_crs(
        "OGC:CRS84"
    )
    _crossings = missed.drop_duplicates("crossing_id")
    _by_step = missed.sort_values("step_m", ignore_index=True)
    _cases = {
        "Shortest step": _by_step.iloc[[0]],
        "Median step": _by_step.iloc[[len(_by_step) // 2]],
        "Longest step": _by_step.iloc[[-1]],
    }

    mo.vstack(
        [
            mo.md(
                f"The {len(_paths):,} kept sessions with a miss, in blue, and the "
                f"{len(_crossings):,} crossings they miss, in orange"
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _paths, get_color=[42, 120, 214], width_min_pixels=1
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        gpd.GeoDataFrame(
                            _crossings[["crossing_id"]],
                            geometry=gpd.points_from_xy(
                                _crossings.lon, _crossings.lat
                            ),
                            crs="OGC:CRS84",
                        ),
                        get_fill_color=[235, 104, 52],
                        radius_min_pixels=4,
                    ),
                ],
                basemap=labelled_basemap(),
            ),
            mo.hstack(
                [
                    mo.vstack(
                        [
                            mo.md(
                                f"**{_name}**: a {_row.step_m.iloc[0]:.0f} m step passes "
                                f"{_row.path_m.iloc[0]:.0f} m from the crossing; the nearest "
                                f"sample is {_row.nearest_sample_m.iloc[0]:.0f} m away"
                            ),
                            miss_map(_row),
                        ]
                    )
                    for _name, _row in _cases.items()
                ],
                widths="equal",
            ),
        ]
    )
    return


@app.cell
def missed_table(missed, mo):
    missed_table = mo.ui.table(
        missed[
            [
                "session_id",
                "crossing_id",
                "path_m",
                "nearest_sample_m",
                "step_m",
                "lon",
                "lat",
            ]
        ]
        .sort_values(["path_m", "step_m"], ignore_index=True)
        .round({"path_m": 1, "nearest_sample_m": 1, "step_m": 1}),
        selection="multi",
        label="Misses, nearest the path first. Select rows to draw them below.",
    )
    missed_table
    return (missed_table,)


@app.cell
def missed_selection_map(miss_map, missed_table, mo):
    _selected = missed_table.value
    (
        miss_map(_selected)
        if len(_selected)
        else mo.md("Select rows in the table above to draw them here.")
    )
    return


if __name__ == "__main__":
    app.run()
