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
def _():
    import a5
    import geopandas as gpd
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import numpy as np
    import pandas as pd
    import polars as pl
    import pyarrow as pa
    import pyproj
    import shapely

    import lookout_evaluation as evaluation
    import lookout_medallion

    return (
        a5,
        evaluation,
        gpd,
        lonboard,
        lookout_medallion,
        mo,
        np,
        pa,
        pl,
        plt,
        pyproj,
        shapely,
    )


@app.cell
def _(evaluation, lookout_medallion, mo, pa):
    picked = pa.table(
        evaluation.pick(
            country="DE", realtime_at_least=90.0, sessions=1000, seed=1
        )
    )
    _ids = ", ".join(
        f"'{_id}'" for _id in picked.column("session_id").to_pylist()
    )
    samples = pa.table(
        lookout_medallion.query_silver(
            "SELECT session_id, t, seq, lon, lat FROM train_session_sample_v3 "
            f"WHERE session_id IN ({_ids}) ORDER BY session_id, t, seq",
            country="DE",
        )
    )
    mo.md(
        f"{picked.num_rows:,} sessions picked, {samples.num_rows:,} samples from "
        f"{len(set(samples.column('session_id').to_pylist())):,} of them"
    )
    return (samples,)


@app.cell
def _(evaluation, mo, pa, samples):
    resolution = 8
    splitter = evaluation.Splitter(
        resolution=resolution, salt="tune-test-1", tune_buckets=70
    )
    split_samples = samples.append_column(
        "split",
        pa.array(
            splitter.splits(
                samples.column("lon").combine_chunks(),
                samples.column("lat").combine_chunks(),
            )
        ),
    )
    _counts = (
        split_samples.group_by("split")
        .aggregate([("session_id", "count")])
        .to_pylist()
    )
    mo.md(
        ", ".join(
            f"{_row['split']}: {_row['session_id_count']:,} samples "
            f"({_row['session_id_count'] / split_samples.num_rows:.0%})"
            for _row in sorted(_counts, key=lambda _row: _row["split"])
        )
    )
    return resolution, split_samples, splitter


@app.cell
def _(mo, pl, pyproj, split_samples):
    geod = pyproj.Geod(ellps="WGS84")

    def metres_between(from_lon, from_lat, to_lon, to_lat):
        _, _, metres = geod.inv(
            from_lon.to_numpy(),
            from_lat.to_numpy(),
            to_lon.to_numpy(),
            to_lat.to_numpy(),
        )
        return pl.Series(metres)

    steps = (
        pl.from_arrow(split_samples)
        .sort("session_id", "t", "seq")
        .with_columns(
            previous_lon=pl.col("lon").shift().over("session_id"),
            previous_lat=pl.col("lat").shift().over("session_id"),
        )
        .with_columns(
            step_metres=pl.when(pl.col("previous_lon").is_not_null()).then(
                pl.struct(
                    "previous_lon", "previous_lat", "lon", "lat"
                ).map_batches(
                    lambda _points: metres_between(
                        _points.struct.field("previous_lon").fill_null(0.0),
                        _points.struct.field("previous_lat").fill_null(0.0),
                        _points.struct.field("lon"),
                        _points.struct.field("lat"),
                    )
                )
            ),
            repeats_position=(pl.col("lon") == pl.col("previous_lon"))
            & (pl.col("lat") == pl.col("previous_lat")),
        )
    )
    mo.vstack(
        [
            mo.md(
                f"{steps.height:,} samples, {steps['step_metres'].null_count():,} of them the first of "
                f"a session, and {steps.filter('repeats_position').height:,} at exactly the "
                "coordinates of the sample before"
            ),
            steps.head(5),
        ]
    )
    return geod, steps


@app.cell
def _(mo, pl, steps):
    horizon_minutes = 5
    horizon = pl.duration(minutes=horizon_minutes)

    starts = (
        steps.with_columns(
            previous_split=pl.col("split").shift().over("session_id")
        )
        .filter(~pl.col("repeats_position"))
        .with_columns(t_until=pl.col("t") + horizon)
        .select(
            "session_id",
            "t",
            "t_until",
            "lon",
            "lat",
            "previous_lon",
            "previous_lat",
            "split",
            "previous_split",
        )
    )
    _later = steps.select(
        pl.col("session_id").alias("session_id_later"),
        pl.col("t").alias("t_later"),
        pl.col("lon").alias("lon_later"),
        pl.col("lat").alias("lat_later"),
        pl.col("split").alias("split_later"),
        pl.col("step_metres").alias("step_metres_later"),
    )
    pairs = starts.join_where(
        _later,
        pl.col("session_id") == pl.col("session_id_later"),
        pl.col("t_later") > pl.col("t"),
        pl.col("t_later") <= pl.col("t_until"),
    ).drop("session_id_later", "t_until")
    _per_start = pairs.group_by("session_id", "t").len()
    mo.md(
        f"{starts.height:,} starts with a heading, {pairs.height:,} pairs of a start and a later "
        f"sample within 5 minutes; a start has {_per_start['len'].median():.0f} later samples at "
        f"the median, from {_per_start['len'].min()} to {_per_start['len'].max()}"
    )
    return horizon_minutes, pairs


@app.cell
def _(mo, pairs, pl):
    dwelling_metres = 500.0

    windows = (
        pairs.group_by("session_id", "t")
        .agg(
            start_lon=pl.col("lon").first(),
            start_lat=pl.col("lat").first(),
            end_lon=pl.col("lon_later").sort_by("t_later").last(),
            end_lat=pl.col("lat_later").sort_by("t_later").last(),
            travelled_metres=pl.col("step_metres_later").sum(),
            wholly_tune=(pl.col("previous_split").first() == "tune")
            & (pl.col("split").first() == "tune")
            & (pl.col("split_later") == "tune").all(),
            wholly_test=(pl.col("previous_split").first() == "test")
            & (pl.col("split").first() == "test")
            & (pl.col("split_later") == "test").all(),
        )
        .with_columns(
            dwelling=pl.col("travelled_metres") < dwelling_metres,
            category=pl.when("wholly_tune")
            .then(pl.lit("tune"))
            .when("wholly_test")
            .then(pl.lit("test"))
            .otherwise(pl.lit("split")),
        )
    )
    _tune = windows.filter("wholly_tune")
    mo.md(
        f"{windows.height:,} windows; {_tune.height:,} lie wholly in tune cells, and "
        f"{windows.height - _tune.height:,} reach into test. Of the tune windows, "
        f"{_tune.filter('dwelling').height:,} travel under {dwelling_metres:,.0f} m and dwell, "
        f"leaving {_tune.filter(~pl.col('dwelling')).height:,}."
    )
    return dwelling_metres, windows


@app.cell
def _(a5, mo, pl, resolution, steps):
    cells = (
        steps.select("lon", "lat", "split")
        .with_columns(
            cell=pl.struct("lon", "lat").map_elements(
                lambda _point: a5.lonlat_to_cell(
                    (_point["lon"], _point["lat"]), resolution
                ),
                return_dtype=pl.UInt64,
            )
        )
        .group_by("cell")
        .agg(samples=pl.len(), splits=pl.col("split").unique())
    )
    _both = cells.filter(pl.col("splits").list.len() > 1)
    cells = cells.with_columns(split=pl.col("splits").list.first()).drop(
        "splits"
    )
    mo.md(
        f"{cells.height:,} cells at resolution {resolution} hold samples: "
        f"{cells.filter(pl.col('split') == 'tune').height:,} tune and "
        f"{cells.filter(pl.col('split') == 'test').height:,} test. "
        f"{_both.height:,} cells hold samples of both splits."
    )
    return (cells,)


@app.cell
def _(a5, cells, gpd, lonboard, mo, pl, shapely, windows):
    _colours = {
        "tune": [31, 119, 180],
        "test": [255, 127, 14],
        "split": [214, 39, 40],
    }
    _cell_shapes = gpd.GeoDataFrame(
        cells.to_pandas(),
        geometry=[
            shapely.Polygon(a5.cell_to_boundary(int(_cell), {"segments": 50}))
            for _cell in cells["cell"]
        ],
        crs="EPSG:4326",
    )
    _window_lines = gpd.GeoDataFrame(
        windows.select("category").to_pandas(),
        geometry=shapely.linestrings(
            windows.select(
                pl.concat_list("start_lon", "start_lat"),
                pl.concat_list("end_lon", "end_lat"),
            )
            .to_numpy()
            .tolist()
        ),
        crs="EPSG:4326",
    )
    mo.vstack(
        [
            mo.md(
                f"{cells.height:,} cells filled by split, under {windows.height:,} windows drawn from "
                "start to last sample "
                '(<span style="color: rgb(31, 119, 180)">■</span> tune, '
                '<span style="color: rgb(255, 127, 14)">■</span> test, '
                '<span style="color: rgb(214, 39, 40)">■</span> split across both)'
            ),
            lonboard.Map(
                [
                    lonboard.PolygonLayer.from_geopandas(
                        _cell_shapes,
                        get_fill_color=lonboard.colormap.apply_categorical_cmap(
                            _cell_shapes.split, _colours, alpha=64
                        ),
                        get_line_color=[80, 80, 80],
                        line_width_min_pixels=1,
                    ),
                    lonboard.PathLayer.from_geopandas(
                        _window_lines,
                        get_color=lonboard.colormap.apply_categorical_cmap(
                            _window_lines.category, _colours
                        ),
                        width_min_pixels=1,
                    ),
                ],
                view_state={"longitude": 10.4, "latitude": 51.2, "zoom": 5.5},
            ),
        ]
    )
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    ## Heading, bearing, and angle

    Each angle compares two directions, both measured clockwise from north in degrees, so north is 0°
    and east is 90°:

    - **Heading** is the direction the train is moving at a window's start: the direction from the
      sample before the start to the start. It is what LightCone will read from the train's last two
      positions.
    - **Bearing** is the direction from the start to one later sample in the window: the way the
      train actually went, to reach that position.

    The **angle** is the difference between the two, from 0° to 180°. A train running straight on
    keeps every bearing close to its heading, and its angles near 0°. A train that turns has
    bearings that drift from its heading, and one that reverses has an angle near 180°. A window's
    largest angle is the half-width a cone around the heading needs to hold every position the train
    reaches within N.
    """)
    return


@app.cell
def _(geod, mo, pairs, pl, windows):
    def bearing_and_metres(from_lon, from_lat, to_lon, to_lat):
        bearing, _, metres = geod.inv(
            from_lon.to_numpy(),
            from_lat.to_numpy(),
            to_lon.to_numpy(),
            to_lat.to_numpy(),
        )
        return pl.DataFrame({"bearing": bearing, "metres": metres}).to_struct()

    _kept = windows.filter(pl.col("wholly_tune") & ~pl.col("dwelling")).select(
        "session_id", "t"
    )
    _measured = (
        pairs.join(_kept, on=["session_id", "t"])
        .with_columns(
            heading=pl.struct(
                "previous_lon", "previous_lat", "lon", "lat"
            ).map_batches(
                lambda _points: bearing_and_metres(
                    *_points.struct.unnest()
                ).struct.field("bearing")
            ),
            to_later=pl.struct(
                "lon", "lat", "lon_later", "lat_later"
            ).map_batches(
                lambda _points: bearing_and_metres(*_points.struct.unnest())
            ),
        )
        .unnest("to_later")
    )
    pair_angles = (
        _measured.filter(
            (pl.col("lon_later") != pl.col("lon"))
            | (pl.col("lat_later") != pl.col("lat"))
        )
        .with_columns(
            angle=(
                (pl.col("bearing") - pl.col("heading") + 180) % 360 - 180
            ).abs()
        )
        .select(
            "session_id",
            "t",
            "t_later",
            "metres",
            "heading",
            "bearing",
            "angle",
        )
    )
    mo.vstack(
        [
            mo.md(
                f"{pair_angles.height:,} pairs of {_kept.height:,} kept windows measured; "
                f"{_measured.height - pair_angles.height:,} pairs left out, whose later sample lies "
                "at the start's position"
            ),
            pair_angles.head(5),
        ]
    )
    return (pair_angles,)


@app.cell
def _(horizon_minutes, mo, np, pair_angles, pl, plt):
    window_angles = pair_angles.group_by("session_id", "t").agg(
        angle=pl.col("angle").max()
    )

    _quantiles = pl.DataFrame(
        {
            "percentile": ["median", "p90", "p99", "p99.9", "largest"],
            "largest angle (°)": [
                window_angles["angle"].quantile(_q)
                for _q in [0.5, 0.9, 0.99, 0.999]
            ]
            + [window_angles["angle"].max()],
        }
    ).with_columns(pl.col("largest angle (°)").round(1))
    _sorted = window_angles["angle"].sort().to_numpy()
    _exceeding = 1 - np.arange(1, len(_sorted) + 1) / len(_sorted)

    _figure, _axes = plt.subplots(figsize=(7, 4))
    _axes.plot(_sorted[:-1], _exceeding[:-1], color="#1f77b4", linewidth=2)
    _axes.set_yscale("log")
    _axes.set_xlim(0, 180)
    _axes.set_xticks(range(0, 181, 30))
    _axes.set_xlabel("Largest angle in a window (°)")
    _axes.set_ylabel("Share of windows exceeding it")
    _axes.set_title(
        f"Largest angle within {horizon_minutes} minutes, over {window_angles.height:,} tune windows"
    )
    _axes.grid(color="#dddddd", linewidth=0.5)
    _axes.spines[["top", "right"]].set_visible(False)

    mo.vstack(
        [
            mo.md(
                f"{window_angles.height:,} windows; "
                f"{(window_angles['angle'] > 90).mean():.2%} need more than 90°, and "
                f"{(window_angles['angle'] > 170).mean():.2%} more than 170°, as a reversal does"
            ),
            mo.hstack([_quantiles, _figure], widths=[1, 2]),
        ]
    )
    return (window_angles,)


@app.cell
def _(gpd, horizon_minutes, lonboard, mo, pairs, pl, shapely, window_angles):
    wide_windows = window_angles.filter(pl.col("angle") > 90)
    _wide_pairs = pairs.join(wide_windows, on=["session_id", "t"])
    _firsts = _wide_pairs.group_by("session_id", "t").first()
    _points = pl.concat(
        [
            _firsts.select(
                "session_id",
                "t",
                order=pl.lit(0),
                lon="previous_lon",
                lat="previous_lat",
            ),
            _firsts.select(
                "session_id", "t", order=pl.lit(1), lon="lon", lat="lat"
            ),
            _wide_pairs.select(
                "session_id",
                "t",
                order=pl.col("t_later")
                .rank()
                .over("session_id", "t")
                .cast(pl.Int32)
                + 1,
                lon="lon_later",
                lat="lat_later",
            ),
        ],
        how="vertical_relaxed",
    )
    _paths = (
        _points.sort("session_id", "t", "order")
        .group_by("session_id", "t", maintain_order=True)
        .agg("lon", "lat")
        .join(wide_windows, on=["session_id", "t"])
    )
    _wide_paths = gpd.GeoDataFrame(
        _paths.select("session_id", "angle").to_pandas(),
        geometry=[
            shapely.LineString(zip(_lon, _lat))
            for _lon, _lat in zip(
                _paths["lon"].to_list(), _paths["lat"].to_list()
            )
        ],
        crs="EPSG:4326",
    )
    _wide_starts = gpd.GeoDataFrame(
        _firsts.select("session_id").to_pandas(),
        geometry=shapely.points(_firsts.select("lon", "lat").to_numpy()),
        crs="EPSG:4326",
    )
    mo.vstack(
        [
            mo.md(
                f"{wide_windows.height:,} windows need more than 90°, from "
                f"{wide_windows['session_id'].n_unique():,} sessions. Each is drawn as its path over "
                f"{horizon_minutes} minutes, from the sample before its start, with its start marked "
                '<span style="color: rgb(255, 127, 14)">●</span>.'
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _wide_paths,
                        get_color=[60, 60, 60, 160],
                        width_min_pixels=1,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _wide_starts,
                        get_fill_color=[255, 127, 14],
                        radius_min_pixels=3,
                    ),
                ],
                view_state={"longitude": 10.4, "latitude": 51.2, "zoom": 5.5},
            ),
        ]
    )
    return


@app.cell
def _(
    dwelling_metres,
    evaluation,
    horizon_minutes,
    mo,
    pa,
    pair_angles,
    pairs,
    pl,
    samples,
    splitter,
    steps,
    window_angles,
    windows,
):
    _stages = evaluation.cone_stages(
        samples=samples,
        split=evaluation.ScoredSplit(splitter=splitter, split="tune"),
        horizon=f"{horizon_minutes}m",
        dwelling_metres=dwelling_metres,
    )
    rust_pairs_without_bearing = _stages.pop("pairs_without_bearing")
    rust_stages = {
        _name: pl.from_arrow(pa.table(_table))
        for _name, _table in _stages.items()
    }
    _notebook = {
        "steps": steps,
        "pairs": pairs,
        "windows": windows,
        "pair_angles": pair_angles,
        "window_angles": window_angles,
    }
    mo.vstack(
        [
            mo.md(
                "The same stages, run by the evaluation's Rust code over the same samples and "
                f"settings; {rust_pairs_without_bearing:,} pairs left out at the start's position"
            ),
            pl.DataFrame(
                {
                    "stage": list(_notebook),
                    "notebook rows": [
                        _frame.height for _frame in _notebook.values()
                    ],
                    "rust rows": [
                        rust_stages[_name].height for _name in _notebook
                    ],
                }
            ),
        ]
    )
    return (rust_stages,)


@app.cell
def _(mo, pl, rust_stages, steps):
    tolerance_metres = 0.001

    _joined = steps.join(
        rust_stages["steps"],
        on=["session_id", "t"],
        how="full",
        suffix="_rust",
        coalesce=True,
    )
    steps_disagreeing = _joined.filter(
        pl.col("lon").is_null()
        | pl.col("lon_rust").is_null()
        | (pl.col("lon") != pl.col("lon_rust"))
        | (pl.col("lat") != pl.col("lat_rust"))
        | ((pl.col("split") == "tune") != pl.col("in_split"))
        | (pl.col("repeats_position") != pl.col("repeats_position_rust"))
        | (
            pl.col("step_metres").is_null()
            != pl.col("step_metres_rust").is_null()
        )
        | (
            (pl.col("step_metres") - pl.col("step_metres_rust")).abs()
            > tolerance_metres
        ).fill_null(False)
    )
    _largest = (
        (_joined["step_metres"] - _joined["step_metres_rust"]).abs().max()
    )
    mo.vstack(
        [
            mo.md(
                f"Steps: {steps_disagreeing.height:,} of {_joined.height:,} samples disagree on "
                "position, split, repeated position, or step distance beyond "
                f"{tolerance_metres * 1000:.0f} mm. The largest step distance difference is "
                f"{_largest:.1e} m."
            ),
            steps_disagreeing.head(5),
        ]
    )
    return (tolerance_metres,)


@app.cell
def _(mo, pairs, rust_stages):
    _keys = ["session_id", "t", "t_later"]
    _rust = rust_stages["pairs"]
    pairs_only_in_notebook = pairs.select(_keys).join(
        _rust, on=_keys, how="anti"
    )
    pairs_only_in_rust = _rust.join(pairs.select(_keys), on=_keys, how="anti")
    mo.vstack(
        [
            mo.md(
                f"Pairs: {pairs_only_in_notebook.height:,} of {pairs.height:,} notebook pairs are "
                f"missing from Rust, and {pairs_only_in_rust.height:,} of {_rust.height:,} Rust "
                "pairs are missing from the notebook."
            ),
            pairs_only_in_notebook.head(5),
            pairs_only_in_rust.head(5),
        ]
    )
    return


@app.cell
def _(mo, pl, rust_stages, tolerance_metres, windows):
    _joined = windows.join(
        rust_stages["windows"],
        on=["session_id", "t"],
        how="full",
        suffix="_rust",
        coalesce=True,
    )
    windows_disagreeing = _joined.filter(
        pl.col("travelled_metres").is_null()
        | pl.col("travelled_metres_rust").is_null()
        | (
            (
                pl.col("travelled_metres") - pl.col("travelled_metres_rust")
            ).abs()
            > tolerance_metres
        )
        | (pl.col("wholly_tune") != pl.col("wholly_in_split"))
        | (pl.col("dwelling") != pl.col("dwelling_rust"))
    )
    _largest = (
        (_joined["travelled_metres"] - _joined["travelled_metres_rust"])
        .abs()
        .max()
    )
    mo.vstack(
        [
            mo.md(
                f"Windows: {windows_disagreeing.height:,} of {_joined.height:,} windows disagree on "
                "whether they lie wholly in tune, whether they dwell, or distance travelled beyond "
                f"{tolerance_metres * 1000:.0f} mm. The largest distance travelled difference is "
                f"{_largest:.1e} m."
            ),
            windows_disagreeing.select(
                "session_id",
                "t",
                "travelled_metres",
                "travelled_metres_rust",
                "wholly_tune",
                "wholly_in_split",
                "dwelling",
                "dwelling_rust",
            ).head(5),
        ]
    )
    return


@app.cell
def _(mo, pair_angles, pl, rust_stages, window_angles):
    tolerance_degrees = 0.001

    def angles_disagreeing(notebook, rust, keys):
        _joined = notebook.select(*keys, "angle").join(
            rust, on=keys, how="full", suffix="_rust", coalesce=True
        )
        return _joined.filter(
            pl.col("angle").is_null()
            | pl.col("angle_rust").is_null()
            | (
                (pl.col("angle") - pl.col("angle_rust")).abs()
                > tolerance_degrees
            )
        ), (_joined["angle"] - _joined["angle_rust"]).abs().max()

    pair_angles_disagreeing, _pairs_largest = angles_disagreeing(
        pair_angles, rust_stages["pair_angles"], ["session_id", "t", "t_later"]
    )
    window_angles_disagreeing, _windows_largest = angles_disagreeing(
        window_angles, rust_stages["window_angles"], ["session_id", "t"]
    )
    mo.vstack(
        [
            mo.md(
                f"Pair angles: {pair_angles_disagreeing.height:,} of {pair_angles.height:,} pairs "
                f"disagree beyond {tolerance_degrees}°, and the largest difference is "
                f"{_pairs_largest:.1e}°. Window angles: {window_angles_disagreeing.height:,} of "
                f"{window_angles.height:,} windows disagree, and the largest difference is "
                f"{_windows_largest:.1e}°."
            ),
            pair_angles_disagreeing.head(5),
            window_angles_disagreeing.head(5),
        ]
    )
    return


if __name__ == "__main__":
    app.run()
