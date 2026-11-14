# /// script
# dependencies = [
#     "geopandas==1.2.0",
#     "lonboard==0.17.0",
#     "lookout-evaluation",
#     "lookout-medallion",
#     "marimo",
#     "matplotlib==3.11.2",
#     "numpy==2.5.4",
#     "pandas==3.1.1",
#     "pya5==1.0.0",
#     "pyarrow==25.0.0",
#     "pyproj==3.7.2",
#     "scipy==1.18.1",
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
    import hashlib
    import pathlib
    import tempfile

    import a5
    import geopandas as gpd
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import numpy as np
    import pandas as pd
    import pyarrow as pa
    import pyproj
    import scipy.stats
    import shapely

    import lookout_evaluation as evaluation
    import lookout_medallion

    return (
        a5,
        evaluation,
        gpd,
        hashlib,
        lonboard,
        lookout_medallion,
        mo,
        np,
        pa,
        pathlib,
        pd,
        plt,
        pyproj,
        scipy,
        shapely,
        tempfile,
    )


@app.cell
def _(lookout_medallion, mo, pa):
    session_routes = pa.table(
        lookout_medallion.query_silver(
            """
            SELECT DISTINCT session.session_id, session.trip_id, segment.route_name, segment.agency_name
            FROM train_session_v3 AS session
            JOIN train_segment_v4 AS segment
                ON segment.source_id = session.source_id AND segment.trip_id = session.trip_id
            WHERE session.realtime_percentage >= 90
            """,
            country="DE",
        )
    ).to_pandas()
    mo.vstack(
        [
            mo.md(
                f"{len(session_routes):,} rows for {session_routes.session_id.nunique():,} sessions, "
                f"{session_routes.route_name.nunique():,} distinct `route_name`s, "
                f"{session_routes.agency_name.isna().sum():,} rows with no agency"
            ),
            session_routes.sample(10, random_state=1),
        ]
    )
    return (session_routes,)


@app.cell
def _(mo, session_routes):
    session_lines = session_routes.assign(
        line=session_routes.route_name.str.split(" (", n=1, regex=False).str[0]
    )
    _agencies_per_line = (
        session_lines.groupby("line").agency_name.nunique().rename("agencies")
    )
    mo.vstack(
        [
            mo.md(
                f"{session_lines.line.nunique():,} lines, "
                f"{session_lines.groupby(['agency_name', 'line'], dropna=False).ngroups:,} (agency, line) pairs, "
                f"{(_agencies_per_line > 1).sum():,} lines run by more than one agency"
            ),
            session_lines[session_lines.session_id.duplicated(keep=False)],
            _agencies_per_line.sort_values(ascending=False).head(10),
        ]
    )
    return (session_lines,)


@app.cell
def _(mo, session_lines):
    line_sessions = (
        session_lines.dropna(subset=["agency_name"])
        .groupby(["agency_name", "line"])
        .session_id.nunique()
        .rename("sessions")
        .reset_index()
        .sort_values(
            ["line", "sessions"], ascending=[True, False], ignore_index=True
        )
    )
    _shown = (
        (line_sessions.line == "RB41")
        | line_sessions.agency_name.eq("Ostdeutsche Eisenbahn GmbH")
        & line_sessions.line.eq("RE1")
        | line_sessions.agency_name.eq("DB Regio AG Nordost")
        & line_sessions.line.eq("FEX")
    )
    line_choice = mo.ui.table(
        line_sessions,
        initial_selection=line_sessions.index[_shown].tolist(),
        label="(agency, line) pairs to draw",
    )
    line_choice
    return (line_choice,)


@app.cell
def _(
    gpd,
    line_choice,
    lonboard,
    lookout_medallion,
    mo,
    pa,
    plt,
    session_lines,
):
    _chosen = session_lines.merge(
        line_choice.value[["agency_name", "line"]],
        on=["agency_name", "line"],
        validate="many_to_one",
    ).assign(pair=lambda _pairs: _pairs.agency_name + " " + _pairs.line)
    _ids = ", ".join(f"'{_id}'" for _id in _chosen.session_id.unique())
    line_paths = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                f"SELECT session_id, geometry FROM train_session_v3 WHERE session_id IN ({_ids})",
                country="DE",
            )
        )
    ).merge(
        _chosen[["session_id", "pair"]], on="session_id", validate="one_to_one"
    )
    _colours = {
        _pair: [
            round(255 * _channel)
            for _channel in plt.get_cmap("tab10")(_index)[:3]
        ]
        for _index, _pair in enumerate(sorted(line_paths.pair.unique()))
    }
    mo.vstack(
        [
            mo.md(
                f"{len(line_paths):,} sessions on {line_paths.pair.nunique()} pairs: "
                + ", ".join(
                    f'<span style="color: rgb{tuple(_colour)}">■</span> {_pair}'
                    for _pair, _colour in _colours.items()
                )
            ),
            lonboard.Map(
                lonboard.PathLayer.from_geopandas(
                    line_paths,
                    get_color=lonboard.colormap.apply_categorical_cmap(
                        line_paths.pair, _colours
                    ),
                    width_min_pixels=2,
                )
            ),
        ]
    )
    return


@app.cell
def _(mo):
    mo.md("""
    ## Splitting sessions by A5 cell

    A split chooses one A5 resolution. A sample's **chunk** is the cell at that resolution that holds
    it, and a **sub-session** is a run of a session's consecutive samples in one chunk. A hash of each
    chunk's id assigns it to the tune set or the test set, so every sub-session falls in exactly one
    set.
    """)
    return


@app.cell
def _(evaluation, lookout_medallion, mo, pa):
    split_picked = pa.table(
        evaluation.pick(
            country="DE", realtime_at_least=90.0, sessions=1000, seed=1
        )
    )
    _ids = ", ".join(
        f"'{_id}'" for _id in split_picked.column("session_id").to_pylist()
    )
    split_samples = pa.table(
        lookout_medallion.query_silver(
            "SELECT session_id, t, seq, lon, lat FROM train_session_sample_v3 "
            f"WHERE session_id IN ({_ids}) ORDER BY session_id, t, seq",
            country="DE",
        )
    ).to_pandas()
    mo.md(
        f"{split_picked.num_rows:,} sessions picked, "
        f"{len(split_samples):,} samples from {split_samples.session_id.nunique():,} of them"
    )
    return split_picked, split_samples


@app.cell
def _(mo):
    chunk_resolution = mo.ui.dropdown(
        options={"7 (2,075 km²)": 7, "8 (519 km²)": 8, "9 (130 km²)": 9},
        value="8 (519 km²)",
        label="Chunk resolution",
    )
    chunk_resolution
    return (chunk_resolution,)


@app.cell
def _(a5, chunk_resolution, mo, split_samples):
    _chunks = [
        a5.lonlat_to_cell((_lon, _lat), chunk_resolution.value)
        for _lon, _lat in zip(split_samples.lon, split_samples.lat)
    ]
    _chunked = split_samples.assign(chunk=_chunks)
    _entered = (_chunked.chunk != _chunked.chunk.shift()) | (
        _chunked.session_id != _chunked.session_id.shift()
    )
    sub_session_samples = _chunked.assign(sub_session=_entered.cumsum())
    sub_sessions = sub_session_samples.groupby("sub_session").agg(
        session_id=("session_id", "first"),
        chunk=("chunk", "first"),
        samples=("t", "size"),
        started_at=("t", "min"),
        ended_at=("t", "max"),
    )
    _minutes = (
        sub_sessions.ended_at - sub_sessions.started_at
    ).dt.total_seconds() / 60
    mo.vstack(
        [
            mo.md(
                f"{len(split_samples):,} samples in {sub_session_samples.chunk.nunique():,} chunks, "
                f"cut into {len(sub_sessions):,} sub-sessions of "
                f"{split_samples.session_id.nunique():,} sessions"
            ),
            _minutes.describe(percentiles=[0.1, 0.5, 0.9])
            .rename("sub-session minutes")
            .to_frame()
            .T,
            sub_sessions.head(10),
        ]
    )
    return sub_session_samples, sub_sessions


@app.cell
def _(a5, hashlib, mo, np, pa, sub_sessions):
    split_salt = "tune-test-1"
    _chunks = np.unique(sub_sessions.chunk.to_numpy().astype(np.uint64))

    def chunk_bucket(salt, chunk):
        return (
            int.from_bytes(
                hashlib.sha256(
                    f"{salt}:{a5.u64_to_hex(int(chunk))}".encode()
                ).digest()[:8]
            )
            % 100
        )

    _buckets = [chunk_bucket(split_salt, _chunk) for _chunk in _chunks]
    chunk_sets = pa.table(
        {
            "chunk": pa.array(_chunks, pa.uint64()),
            "bucket": _buckets,
            "set": [
                "tune" if _bucket < 70 else "test" for _bucket in _buckets
            ],
        }
    ).to_pandas()
    sub_session_sets = (
        sub_sessions.assign(chunk=sub_sessions.chunk.astype(np.uint64))
        .reset_index()
        .merge(
            chunk_sets[["chunk", "set"]], on="chunk", validate="many_to_one"
        )
        .set_index("sub_session")
    )
    mo.vstack(
        [
            mo.md(f"Salt `{split_salt}`: 70 of 100 buckets go to tune"),
            sub_session_sets.groupby("set")
            .agg(
                chunks=("chunk", "nunique"),
                sub_sessions=("chunk", "size"),
                sessions=("session_id", "nunique"),
                samples=("samples", "sum"),
            )
            .assign(
                sample_share=lambda _sets: _sets.samples / _sets.samples.sum()
            ),
        ]
    )
    return chunk_bucket, chunk_sets, split_salt, sub_session_sets


@app.cell
def _(
    a5,
    chunk_resolution,
    chunk_sets,
    gpd,
    lonboard,
    mo,
    np,
    shapely,
    sub_session_samples,
    sub_session_sets,
):
    _set_colours = {"tune": [31, 119, 180], "test": [255, 127, 14]}
    _drawn = sub_session_samples.merge(
        sub_session_sets[["set"]],
        left_on="sub_session",
        right_index=True,
        validate="many_to_one",
    )
    _misdrawn = (
        _drawn.set
        != _drawn.chunk.astype(np.uint64).map(
            dict(zip(chunk_sets.chunk, chunk_sets.set))
        )
    ).sum()
    _drawn = _drawn[
        _drawn.groupby("sub_session").sub_session.transform("size") > 1
    ]
    _paths = gpd.GeoDataFrame(
        _drawn.groupby("sub_session", sort=True).agg(
            session_id=("session_id", "first"), set=("set", "first")
        ),
        geometry=shapely.linestrings(
            _drawn[["lon", "lat"]].to_numpy(),
            indices=_drawn.sub_session.rank(method="dense")
            .astype(int)
            .to_numpy()
            - 1,
        ),
        crs="EPSG:4326",
    )
    _chunk_shapes = gpd.GeoDataFrame(
        chunk_sets,
        geometry=[
            shapely.Polygon(a5.cell_to_boundary(int(_chunk), {"segments": 50}))
            for _chunk in chunk_sets.chunk
        ],
        crs="EPSG:4326",
    )
    mo.vstack(
        [
            mo.md(
                f"Chunks at resolution {chunk_resolution.value}, filled by set "
                '(<span style="color: rgb(31, 119, 180)">■</span> tune, '
                '<span style="color: rgb(255, 127, 14)">■</span> test), '
                f"under {len(_paths):,} sub-sessions of two or more samples. "
                f"{_misdrawn:,} samples are drawn in a set other than their chunk's."
            ),
            lonboard.Map(
                [
                    lonboard.PolygonLayer.from_geopandas(
                        _chunk_shapes,
                        get_fill_color=lonboard.colormap.apply_categorical_cmap(
                            _chunk_shapes.set, _set_colours, alpha=64
                        ),
                        get_line_color=[80, 80, 80],
                        line_width_min_pixels=1,
                    ),
                    lonboard.PathLayer.from_geopandas(
                        _paths,
                        get_color=lonboard.colormap.apply_categorical_cmap(
                            _paths.set, _set_colours
                        ),
                        width_min_pixels=2,
                    ),
                ],
                view_state={"longitude": 10.4, "latitude": 51.2, "zoom": 5.5},
            ),
        ]
    )
    return


@app.cell
def _(mo):
    mo.md("""
    ### Checking the chunks against Germany's outline

    Every sample of a kept session lies within Germany's land or maritime area. The cells at the chunk
    resolution that overlap that area must therefore hold every chunk the split found. The cover comes
    from `polygon_to_cells`, a second derivation that shares nothing with the samples.
    """)
    return


@app.cell
def _(gpd, lookout_medallion, pa, shapely):
    _areas = pa.table(
        lookout_medallion.query_bronze(
            "SELECT extract_id, class, geometry FROM division_area "
            "WHERE division_id = $division_id "
            "AND extract_id = (SELECT MAX(extract_id) FROM division_area WHERE division_id = $division_id)",
            tables={
                "division_area": {
                    "theme": "divisions",
                    "type": "division_area",
                }
            },
            params={"division_id": lookout_medallion.division_id("DE")},
        )
    ).to_pandas()
    germany_areas = gpd.GeoDataFrame(
        _areas[["extract_id", "class"]],
        geometry=shapely.from_wkb(_areas.geometry),
        crs="EPSG:4326",
    )
    germany_areas.assign(
        polygons=shapely.get_num_geometries(germany_areas.geometry),
        points=shapely.get_num_coordinates(germany_areas.geometry),
    )[["extract_id", "class", "polygons", "points"]]
    return (germany_areas,)


@app.cell
def _(a5, chunk_resolution, chunk_sets, germany_areas, mo, shapely):
    _polygons = [
        _polygon
        for _area in germany_areas.geometry
        for _polygon in shapely.get_parts(_area)
    ]
    germany_cells = set()
    for _polygon in _polygons:
        germany_cells.update(
            a5.uncompact(
                a5.polygon_to_cells(
                    [list(_polygon.exterior.coords)]
                    + [list(_hole.coords) for _hole in _polygon.interiors],
                    chunk_resolution.value,
                    {"containment": "overlapping"},
                )
            )
        )
    _found = set(int(_chunk) for _chunk in chunk_sets.chunk)
    _outside = _found - germany_cells
    mo.md(
        f"Resolution {chunk_resolution.value}: {len(germany_cells):,} cells overlap Germany's "
        f"{len(_polygons):,} polygons. The split found {len(_found):,} chunks, "
        f"**{len(_outside):,} of them outside that cover**."
    )
    return


@app.cell
def _(mo):
    mo.md("""
    ### What a sub-session's edges cost

    A pass in its sub-session's first minute goes unscored, since a predictor first needs samples to
    measure a speed. A pass under N after its sub-session starts has an alert window that reaches back
    into the previous chunk, so its notification falls in another sub-session, and possibly in the other
    set. Whole sessions give the baseline: they lose only their own first minute.
    """)
    return


@app.cell
def _(lookout_medallion, mo, pa, split_picked):
    _ids = ", ".join(
        f"'{_id}'" for _id in split_picked.column("session_id").to_pylist()
    )
    split_passes = pa.table(
        lookout_medallion.query_silver(
            "SELECT session_id, crossing_id, crossed_at FROM train_session_crossing_v1 "
            f"WHERE session_id IN ({_ids})"
        )
    ).to_pandas()
    mo.md(
        f"{len(split_passes):,} passes in {split_passes.session_id.nunique():,} of the "
        f"{split_picked.num_rows:,} sessions"
    )
    return (split_passes,)


@app.cell
def _(a5, pd, split_passes, split_samples):
    def _pass_offsets(samples, piece):
        _pieces = (
            samples.assign(piece=piece).groupby("piece").t.agg(["min", "max"])
        )
        _at = split_passes.merge(
            samples.assign(piece=piece)[["session_id", "t", "piece"]],
            left_on=["session_id", "crossed_at"],
            right_on=["session_id", "t"],
            validate="many_to_many",
        ).drop_duplicates(["session_id", "crossing_id", "crossed_at"])
        return (
            _at.crossed_at - _at.piece.map(_pieces["min"])
        ).dt.total_seconds() / 60

    def _sub_session_of(resolution):
        _chunk = pd.Series(
            [
                a5.lonlat_to_cell((_lon, _lat), resolution)
                for _lon, _lat in zip(split_samples.lon, split_samples.lat)
            ],
            index=split_samples.index,
        )
        return (
            (_chunk != _chunk.shift())
            | (split_samples.session_id != split_samples.session_id.shift())
        ).cumsum()

    _pieces = {"whole sessions": split_samples.session_id} | {
        f"resolution {_resolution}": _sub_session_of(_resolution)
        for _resolution in (7, 8, 9)
    }
    edge_costs = pd.DataFrame(
        {
            _name: {
                "pieces": _piece.nunique(),
                "passes matched": len(
                    _minutes := _pass_offsets(split_samples, _piece)
                ),
                "in first minute": (_minutes < 1).mean(),
                "under N after start": (_minutes < 5).mean(),
            }
            for _name, _piece in _pieces.items()
        }
    ).T
    edge_costs
    return


@app.cell
def _(mo):
    mo.md("""
    ### Do the sets have the same shape of track?

    LightCone reads how far trains turn, so the tune set and the test set need track of the same shape.
    Two measures describe a window of N from each sample:

    - **Straightness** is the straight-line distance from the window's first sample to its last, over
      the distance travelled between them. A straight run scores 1, and a winding one less.
      [Benhamou](https://doi.org/10.1016/j.jtbi.2004.03.016) recommends it for oriented paths, such as
      a train's. Its value depends on the window, so every window spans N.
    - **Heading angle** is the angle between a sample's heading, from the sample before it, and the
      bearing to its position N later. It is what LightCone's cone bounds.

    A window counts towards a set only when all its samples lie in that set's chunks. A window whose
    train travels under 500 m is dwelling, and is left out.
    """)
    return


@app.cell
def _(chunk_sets, mo, np, pd, pyproj, sub_session_samples):
    _geod = pyproj.Geod(ellps="WGS84")
    _samples = sub_session_samples.reset_index(drop=True)
    _same_session = _samples.session_id.eq(_samples.session_id.shift())
    _heading, _, _step = _geod.inv(
        _samples.lon.shift().to_numpy(),
        _samples.lat.shift().to_numpy(),
        _samples.lon.to_numpy(),
        _samples.lat.to_numpy(),
    )
    _step = np.where(_same_session, _step, 0.0)
    _travelled = (
        pd.Series(_step).groupby(_samples.session_id).cumsum().to_numpy()
    )
    _seconds = _samples.t.dt.as_unit("s").astype("int64").to_numpy()
    _session_codes = _samples.session_id.factorize()[0]
    _key = _session_codes.astype(np.int64) * 10**11 + _seconds
    _ends = np.searchsorted(_key, _key + 300)
    _valid = (_ends < len(_samples)) & _same_session.to_numpy()
    _ends = np.minimum(_ends, len(_samples) - 1)
    _valid &= _session_codes[_ends] == _session_codes
    _bearing, _, _chord = _geod.inv(
        _samples.lon.to_numpy(),
        _samples.lat.to_numpy(),
        _samples.lon.to_numpy()[_ends],
        _samples.lat.to_numpy()[_ends],
    )
    _path = _travelled[_ends] - _travelled
    _valid &= _path >= 500
    window_bounds = pd.DataFrame(
        {
            "start": np.arange(len(_samples)),
            "end": _ends,
            "lon": _samples.lon.to_numpy(),
            "lat": _samples.lat.to_numpy(),
            "straightness": np.divide(
                _chord, _path, out=np.full(len(_path), np.nan), where=_path > 0
            ),
            "heading_angle": np.abs((_bearing - _heading + 180) % 360 - 180),
            "path_metres": _path,
        }
    )[_valid & (_step > 0)]
    _sample_chunks = _samples.chunk.astype(np.uint64)
    _new_session = ~_same_session.to_numpy()

    def windows_in_sets(set_of_chunk):
        _set = _sample_chunks.map(set_of_chunk).to_numpy()
        _runs = np.cumsum(_new_session | (_set != np.roll(_set, 1)))
        _kept = window_bounds[
            _runs[window_bounds.start] == _runs[window_bounds.end]
        ]
        return _kept.assign(set=_set[_kept.start])

    shape_windows = windows_in_sets(
        dict(zip(chunk_sets.chunk, chunk_sets.set))
    )
    mo.md(
        f"{len(window_bounds):,} windows of N from {len(_samples):,} samples, "
        f"{len(shape_windows):,} of them within one set: "
        + ", ".join(
            f"{_count:,} in {_set}"
            for _set, _count in shape_windows.set.value_counts().items()
        )
    )
    return shape_windows, window_bounds, windows_in_sets


@app.cell
def _(mo, pd, plt, scipy, shape_windows):
    _measures = {
        "straightness": "Straightness",
        "heading_angle": "Heading angle (°)",
    }
    _figure, _axes = plt.subplots(1, 2, figsize=(11, 3.5))
    for _axis, (_column, _label) in zip(_axes, _measures.items()):
        for _set, _colour in (("tune", "#1f77b4"), ("test", "#ff7f0e")):
            _axis.hist(
                shape_windows.loc[shape_windows.set == _set, _column],
                bins=60,
                density=True,
                histtype="step",
                color=_colour,
                label=_set,
            )
        _axis.set_xlabel(_label)
        _axis.set_yscale("log")
        _axis.legend()
    _figure.tight_layout()
    shape_by_set = pd.DataFrame(
        {
            _label: {
                **{
                    f"{_set} p{_q}": shape_windows.loc[
                        shape_windows.set == _set, _column
                    ].quantile(_q / 100)
                    for _q in (10, 50, 90, 99)
                    for _set in ("tune", "test")
                },
                "KS statistic": scipy.stats.ks_2samp(
                    shape_windows.loc[shape_windows.set == "tune", _column],
                    shape_windows.loc[shape_windows.set == "test", _column],
                ).statistic,
            }
            for _column, _label in _measures.items()
        }
    )
    mo.vstack([shape_by_set, _figure])
    return


@app.cell
def _(mo):
    mo.md("""
    #### How much the sets differ under other salts

    Each salt splits the same chunks another way. The spread of each measure over 21 salts shows how far
    two sets differ when chunks are split at random, so the chosen salt's sets can be judged against it.
    """)
    return


@app.cell
def _(
    chunk_bucket,
    chunk_sets,
    mo,
    np,
    pd,
    plt,
    scipy,
    split_salt,
    windows_in_sets,
):
    def _compare(salt):
        _windows = windows_in_sets(
            {
                _chunk: "tune" if chunk_bucket(salt, _chunk) < 70 else "test"
                for _chunk in chunk_sets.chunk
            }
        )
        _tune = _windows[_windows.set == "tune"]
        _test = _windows[_windows.set == "test"]
        return {
            "salt": salt,
            "test share of windows": len(_test) / len(_windows),
            "straightness KS": scipy.stats.ks_2samp(
                _tune.straightness, _test.straightness
            ).statistic,
            "heading angle KS": scipy.stats.ks_2samp(
                _tune.heading_angle, _test.heading_angle
            ).statistic,
            "heading angle p90 gap": _test.heading_angle.quantile(0.9)
            - _tune.heading_angle.quantile(0.9),
            "heading angle p99 gap": _test.heading_angle.quantile(0.99)
            - _tune.heading_angle.quantile(0.99),
        }

    salt_spread = pd.DataFrame(
        [_compare(f"tune-test-{_n}") for _n in range(1, 22)]
    ).set_index("salt")
    _figure, _axes = plt.subplots(1, 5, figsize=(14, 3))
    for _axis, _column in zip(_axes, salt_spread.columns):
        _axis.scatter(
            np.zeros(len(salt_spread)),
            salt_spread[_column],
            color="#999999",
            s=12,
        )
        _axis.scatter(
            [0], [salt_spread.loc[split_salt, _column]], color="#d62728", s=30
        )
        _axis.set_title(_column, fontsize=9)
        _axis.set_xticks([])
    _figure.tight_layout()
    mo.vstack(
        [
            mo.md(f"Red marks `{split_salt}`, grey the other 20 salts"),
            _figure,
            salt_spread.describe().loc[["min", "50%", "max"]],
            salt_spread.loc[[split_salt]],
        ]
    )
    return


@app.cell
def _(
    a5,
    chunk_sets,
    gpd,
    lonboard,
    mo,
    np,
    shape_windows,
    shapely,
    sub_session_samples,
):
    _threshold = shape_windows.heading_angle.quantile(0.99)
    _sharp = shape_windows[shape_windows.heading_angle >= _threshold]
    _sharp_chunks = (
        _sharp.groupby(
            _sharp.start.map(
                sub_session_samples.reset_index(drop=True).chunk
            ).astype(np.uint64)
        )
        .size()
        .rename("windows")
        .sort_values(ascending=False)
    )
    _sharp_points = gpd.GeoDataFrame(
        _sharp[["set", "heading_angle"]],
        geometry=shapely.points(_sharp.lon, _sharp.lat),
        crs="EPSG:4326",
    )
    _chunk_outlines = gpd.GeoDataFrame(
        chunk_sets,
        geometry=[
            shapely.Polygon(a5.cell_to_boundary(int(_chunk), {"segments": 50}))
            for _chunk in chunk_sets.chunk
        ],
        crs="EPSG:4326",
    )
    _set_colours = {"tune": [31, 119, 180], "test": [255, 127, 14]}
    mo.vstack(
        [
            mo.md(
                f"{len(_sharp):,} windows turn through {_threshold:.0f}° or more, the p99. "
                f"{len(_sharp_chunks):,} chunks hold them, and the 5 busiest hold "
                f"{_sharp_chunks.head(5).sum() / len(_sharp):.0%}."
            ),
            lonboard.Map(
                [
                    lonboard.PolygonLayer.from_geopandas(
                        _chunk_outlines,
                        get_fill_color=lonboard.colormap.apply_categorical_cmap(
                            _chunk_outlines.set, _set_colours, alpha=40
                        ),
                        get_line_color=[80, 80, 80],
                        line_width_min_pixels=1,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _sharp_points,
                        get_fill_color=lonboard.colormap.apply_categorical_cmap(
                            _sharp_points.set, _set_colours
                        ),
                        radius_min_pixels=3,
                    ),
                ],
                view_state={"longitude": 10.4, "latitude": 51.2, "zoom": 5.5},
            ),
            _sharp_chunks.head(10).to_frame(),
        ]
    )
    return


@app.cell
def _(mo):
    mo.md("""
    ### Experiment 7: does cutting sessions change a score?

    CrowFlies at 1 km runs over the test set twice. **Cut**: each test sub-session replays as a session
    of its own, from a scratch store holding the sub-sessions, their passes, and the crossings. **By
    chunk**: each whole session replays, and only the notifications and passes whose crossing lies in a
    test chunk are scored. CrowFlies tunes nothing, so any gap between the two comes from the method.
    False alarms per hour divide by the hours spent in test chunks, in both.
    """)
    return


@app.cell
def _(
    chunk_sets,
    lookout_medallion,
    mo,
    np,
    pa,
    pathlib,
    split_picked,
    sub_session_samples,
    tempfile,
):
    _ids = ", ".join(
        f"'{_id}'" for _id in split_picked.column("session_id").to_pylist()
    )
    _samples = pa.table(
        lookout_medallion.query_silver(
            "SELECT * FROM train_session_sample_v3 "
            f"WHERE session_id IN ({_ids}) ORDER BY session_id, t, seq",
            country="DE",
        )
    ).to_pandas()
    assert (
        _samples.session_id.to_numpy()
        == sub_session_samples.session_id.to_numpy()
    ).all()
    assert (_samples.t.to_numpy() == sub_session_samples.t.to_numpy()).all()
    _set_of_chunk = dict(zip(chunk_sets.chunk, chunk_sets.set))
    _cut = _samples.assign(
        parent=_samples.session_id,
        session_id=_samples.session_id
        + "_"
        + sub_session_samples.sub_session.astype(str).to_numpy(),
        set=sub_session_samples.chunk.astype(np.uint64)
        .map(_set_of_chunk)
        .to_numpy(),
    )
    _cut = _cut[_cut.set == "test"]
    _pieces = _cut.groupby("session_id").agg(
        parent=("parent", "first"),
        started_at=("t", "min"),
        ended_at=("t", "max"),
        sample_count=("t", "size"),
    )
    _pieces = _pieces[_pieces.sample_count > 1]
    _cut = _cut[_cut.session_id.isin(_pieces.index)]
    cut_sessions = pa.Table.from_pandas(
        split_picked.to_pandas()
        .drop(columns=["started_at", "ended_at", "sample_count"])
        .merge(
            _pieces.reset_index(),
            left_on="session_id",
            right_on="parent",
            suffixes=("_whole", ""),
        )
        .drop(columns=["session_id_whole", "parent"])[
            split_picked.column_names
        ],
        schema=split_picked.schema,
        preserve_index=False,
    )
    _passes = pa.table(
        lookout_medallion.query_silver(
            f"SELECT * FROM train_session_crossing_v1 WHERE session_id IN ({_ids})"
        )
    ).to_pandas()
    _cut_passes = _passes.merge(
        _cut[["parent", "t", "session_id"]],
        left_on=["session_id", "crossed_at"],
        right_on=["parent", "t"],
        suffixes=("_whole", ""),
        validate="many_to_one",
    )[_passes.columns]
    _crossings = pa.table(
        lookout_medallion.query_silver(
            "SELECT * FROM water_crossing_v2", country="DE"
        )
    )
    cut_root = pathlib.Path(tempfile.mkdtemp(prefix="explore6-cut-"))
    lookout_medallion.write_silver(
        "water_crossing_v2",
        _crossings.append_column(
            "country", pa.array(["DE"] * _crossings.num_rows)
        ),
        root=cut_root,
    )
    lookout_medallion.write_silver(
        "train_session_sample_v3",
        pa.Table.from_pandas(
            _cut.drop(columns=["parent", "set"]).assign(country="DE"),
            preserve_index=False,
        ),
        root=cut_root,
    )
    lookout_medallion.write_silver(
        "train_session_crossing_v1",
        pa.Table.from_pandas(_cut_passes, preserve_index=False),
        root=cut_root,
    )
    test_hours = (
        (_pieces.ended_at - _pieces.started_at).dt.total_seconds() / 3600
    ).sum()
    mo.md(
        f"{cut_sessions.num_rows:,} test sub-sessions of two or more samples, "
        f"{len(_cut):,} samples, {len(_cut_passes):,} passes, {test_hours:,.1f} hours in test chunks"
    )
    return cut_root, cut_sessions, test_hours


@app.cell
def _(
    a5,
    chunk_bucket,
    chunk_resolution,
    cut_root,
    cut_sessions,
    evaluation,
    lookout_medallion,
    pa,
    pd,
    shapely,
    split_picked,
    split_salt,
    test_hours,
):
    _crow_flies = evaluation.CrowFlies(radius_metres=1000.0, horizon="5m")
    _scoring = evaluation.Scoring(
        horizon="5m",
        last_moment="30s",
        ramp="1m",
        last_moment_score=0.25,
        floor=0.05,
        warm_up="1m",
    )
    _bootstrap = evaluation.Bootstrap(resamples=1000, seed=1)

    _cut_transitions = pa.table(
        evaluation.replay(
            country="DE",
            sessions=cut_sessions,
            predictor=_crow_flies,
            root=cut_root,
        )
    )
    _cut_outcomes = pa.table(
        evaluation.score(
            country="DE",
            sessions=cut_sessions,
            transitions=_cut_transitions,
            scoring=_scoring,
            root=cut_root,
        )
    )

    _whole_transitions = pa.table(
        evaluation.replay(
            country="DE", sessions=split_picked, predictor=_crow_flies
        )
    )
    _whole_outcomes = pa.table(
        evaluation.score(
            country="DE",
            sessions=split_picked,
            transitions=_whole_transitions,
            scoring=_scoring,
        )
    )
    _crossings = pa.table(
        lookout_medallion.query_silver(
            "SELECT crossing_compact_id, geometry FROM water_crossing_v2",
            country="DE",
        )
    ).to_pandas()
    _centres = shapely.centroid(shapely.from_wkb(_crossings.geometry))
    _crossing_set = dict(
        zip(
            _crossings.crossing_compact_id,
            [
                "test"
                if chunk_bucket(
                    split_salt,
                    a5.lonlat_to_cell((_x, _y), chunk_resolution.value),
                )
                >= 70
                else "tune"
                for _x, _y in zip(
                    shapely.get_x(_centres), shapely.get_y(_centres)
                )
            ],
        )
    )
    _test_outcomes = _whole_outcomes.filter(
        pa.array(
            [
                _crossing_set[_id] == "test"
                for _id in _whole_outcomes.column(
                    "crossing_compact_id"
                ).to_pylist()
            ]
        )
    )

    def _measures(sessions, outcomes):
        _summary = evaluation.summarise(
            sessions=sessions, outcomes=outcomes, bootstrap=_bootstrap
        )
        _row = pa.table(_summary).to_pylist()[0]
        return {
            "sessions": _row["sessions"],
            "passes": _row["passes"],
            "notifications": _row["notifications"],
            "false alarms": _row["false_alarms"],
            "misses": _row["misses"],
            "precision": f"{_row['precision']:.3f} [{_row['precision_low']:.3f}, {_row['precision_high']:.3f}]",
            "recall": f"{_row['recall']:.3f} [{_row['recall_low']:.3f}, {_row['recall_high']:.3f}]",
            "false alarms per test hour": round(
                _row["false_alarms"] / test_hours, 1
            ),
            "lead p10 / p50 / p90 s": " / ".join(
                f"{_row[_key] / 1000:.0f}"
                for _key in ("lead_p10_ms", "lead_median_ms", "lead_p90_ms")
            ),
        }

    cut_or_chunk = pd.DataFrame(
        {
            "cut sub-sessions": _measures(cut_sessions, _cut_outcomes),
            "whole sessions, by crossing's chunk": _measures(
                split_picked, _test_outcomes
            ),
        }
    )
    cut_or_chunk
    return


@app.cell
def _(mo):
    mo.md("""
    #### The spread over salts at each resolution

    The same 21 salts split the chunks at resolutions 7, 8, and 9. A finer resolution spreads the
    reversals over more chunks, but more windows cross from one set into the other and are dropped.
    """)
    return


@app.cell
def _(a5, chunk_bucket, np, pd, scipy, split_samples, window_bounds):
    _new_session = (
        split_samples.session_id != split_samples.session_id.shift()
    ).to_numpy()
    _threshold = window_bounds.heading_angle.quantile(0.99)

    def _spread_at(resolution):
        _chunks = pd.Series(
            [
                a5.lonlat_to_cell((_lon, _lat), resolution)
                for _lon, _lat in zip(split_samples.lon, split_samples.lat)
            ]
        )
        _unique = _chunks.unique()
        _rows = []
        for _n in range(1, 22):
            _salt = f"tune-test-{_n}"
            _set_of = {
                _chunk: chunk_bucket(_salt, _chunk) < 70 for _chunk in _unique
            }
            _tune = _chunks.map(_set_of).to_numpy()
            _runs = np.cumsum(_new_session | (_tune != np.roll(_tune, 1)))
            _kept = window_bounds[
                _runs[window_bounds.start] == _runs[window_bounds.end]
            ]
            _kept_tune = _tune[_kept.start]
            _rows.append(
                {
                    "windows kept": len(_kept) / len(window_bounds),
                    "test share of windows": 1 - _kept_tune.mean(),
                    "straightness KS": scipy.stats.ks_2samp(
                        _kept.straightness[_kept_tune],
                        _kept.straightness[~_kept_tune],
                    ).statistic,
                    "heading angle KS": scipy.stats.ks_2samp(
                        _kept.heading_angle[_kept_tune],
                        _kept.heading_angle[~_kept_tune],
                    ).statistic,
                    "heading angle p99 gap": _kept.heading_angle[
                        ~_kept_tune
                    ].quantile(0.99)
                    - _kept.heading_angle[_kept_tune].quantile(0.99),
                }
            )
        _sharp = _chunks[
            window_bounds.start[window_bounds.heading_angle >= _threshold]
        ].value_counts()
        _spread = pd.DataFrame(_rows)
        return {
            "chunks": len(_unique),
            "chunks holding p99 windows": len(_sharp),
            "busiest 5 chunks' share of p99 windows": _sharp.head(5).sum()
            / _sharp.sum(),
            **{
                f"{_column} ({_stat})": getattr(_spread[_column], _stat)()
                for _column in _spread.columns
                for _stat in ("min", "median", "max")
            },
        }

    resolution_spread = pd.DataFrame(
        {f"resolution {_r}": _spread_at(_r) for _r in (7, 8, 9)}
    )
    resolution_spread
    return


if __name__ == "__main__":
    app.run()
