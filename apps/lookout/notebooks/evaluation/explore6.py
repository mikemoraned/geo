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

    import a5
    import geopandas as gpd
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import numpy as np
    import pyarrow as pa
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
        plt,
        shapely,
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
    return (split_samples,)


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
    _buckets = [
        int.from_bytes(
            hashlib.sha256(
                f"{split_salt}:{a5.u64_to_hex(int(_chunk))}".encode()
            ).digest()[:8]
        )
        % 100
        for _chunk in _chunks
    ]
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
    return chunk_sets, sub_session_sets


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


if __name__ == "__main__":
    app.run()
