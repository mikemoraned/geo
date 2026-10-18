# /// script
# dependencies = [
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo",
#     "matplotlib==3.11.1",
#     "numpy==2.5.4",
#     "pandas==3.0.6",
#     "polyline==2.0.2",
#     "pyarrow==25.0.0",
#     "shapely==2.1.2",
# ]
# requires-python = ">=3.13"
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.1"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import geopandas as gpd
    import lonboard
    import matplotlib.colors as mcolors
    import matplotlib.dates as mdates
    import matplotlib.pyplot as plt
    import numpy as np
    import pandas as pd
    import polyline
    import pyarrow as pa
    import shapely
    from lonboard.colormap import apply_continuous_cmap
    from shapely.geometry import LineString

    import lookout_medallion

    return (
        LineString,
        apply_continuous_cmap,
        gpd,
        lonboard,
        lookout_medallion,
        mcolors,
        mdates,
        mo,
        np,
        pa,
        pd,
        plt,
        polyline,
        shapely,
    )


@app.cell
def _(mo):
    mo.md(r"""
    # Germany: `delfi-2026-10-10.route-shapes.within`

    Every mainline and regional train in Germany, captured from the local Motis server every 10
    minutes over a day. The import computed rail shapes along OSM track with `route_shapes`, from the
    DELFI build of 10 October 2026. The capture keeps only the legs lying wholly within Germany.

    Each step below adds one named query to those before it, and shows what it returns.
    """)
    return


@app.cell
def _(LineString, gpd, lookout_medallion, pa, pd, polyline):
    MEDALLION_ROOT = lookout_medallion.default_root()
    LABEL = "delfi-2026-10-10.route-shapes.within"
    COUNTRY = "DE"
    POLYLINE_PRECISION = 5
    # Motis encodes each shape as a Google polyline, whose points are WGS 84 latitude and longitude.
    POLYLINE_CRS = "EPSG:4326"
    # Edge lengths need a CRS in metres. The store projects German data to ETRS89 / UTM zone 32N.
    METRIC_CRS = lookout_medallion.projected_crs(COUNTRY)

    def bronze(
        sql: str,
        steps: tuple[str, ...] = (),
        tables: dict[str, dict[str, str]] | None = None,
        **params: str,
    ) -> pa.Table:
        if steps:
            sql = "WITH " + ",\n".join(steps) + "\n" + sql
        return pa.table(
            lookout_medallion.query_bronze(
                sql,
                tables=tables,
                params=dict(label=LABEL, country=COUNTRY, **params),
                root=str(MEDALLION_ROOT),
            )
        )

    def shape_of(encoded: pd.Series) -> gpd.GeoSeries:
        return gpd.GeoSeries(
            [
                LineString(
                    polyline.decode(line, POLYLINE_PRECISION, geojson=True)
                )
                for line in encoded
            ],
            index=encoded.index,
            crs=POLYLINE_CRS,
        )

    return METRIC_CRS, bronze, shape_of


@app.cell
def _(mo):
    mo.md(r"""
    ## The capture

    The source is the local server's, carrying the label and covering the whole country, with no
    region.
    """)
    return


@app.cell
def _(bronze):
    SOURCE = (
        """
    source AS (
        SELECT source_id FROM motis_source
        WHERE feed = 'local' AND label = $label AND country = $country AND region_id IS NULL
    )""",
    )
    bronze(
        """
        SELECT source_id, base_url, motis_version, overture_release, registered_at
        FROM source JOIN motis_source USING (source_id)
        """,
        SOURCE,
    )
    return (SOURCE,)


@app.cell
def _(mo):
    mo.md(r"""
    `capture` holds every row the source captured, each with its poll: the capture instant rounded
    to the minute.
    """)
    return


@app.cell
def _(SOURCE, bronze):
    CAPTURE = SOURCE + (
        """
    capture AS (
        SELECT
            date_trunc('minute', captured_at + INTERVAL '30 seconds') AS poll,
            trip_id, route_name, mode, from_stop_id, to_stop_id, scheduled_departure, polyline
        FROM motis_segment_v2 JOIN source USING (source_id)
    )""",
    )
    bronze(
        """
        SELECT count(DISTINCT poll) AS polls, min(poll) AS first_poll, max(poll) AS last_poll,
               count(*) AS captured_rows
        FROM capture
        """,
        CAPTURE,
    )
    return (CAPTURE,)


@app.cell
def _(mo):
    mo.md(r"""
    ## Trains per poll

    A train is one trip. Each poll counts the distinct trips it saw running, by mode.
    """)
    return


@app.cell
def _(CAPTURE, bronze):
    trains_per_poll = (
        bronze(
            """
            SELECT poll, mode, count(DISTINCT trip_id) AS trains
            FROM capture GROUP BY poll, mode ORDER BY poll, mode
            """,
            CAPTURE,
        )
        .to_pandas()
        .pivot(index="poll", columns="mode", values="trains")
        .fillna(0)
        .astype(int)
    )
    trains_per_poll
    return (trains_per_poll,)


@app.cell
def _(mdates, plt, trains_per_poll):
    _modes = trains_per_poll.sum().sort_values(ascending=False).index
    _colours = ["#2a78d6", "#1baf7a", "#eb6834", "#e87ba4"][: len(_modes)]
    _fig, _ax = plt.subplots(figsize=(11, 3.6))
    _ax.stackplot(
        trains_per_poll.index,
        [trains_per_poll[_mode] for _mode in _modes],
        labels=_modes,
        colors=_colours,
        edgecolor="white",
        linewidth=1,
    )
    _ax.set_ylabel("trains running")
    _ax.grid(axis="y", color="#e4e3df", linewidth=0.8)
    _ax.tick_params(colors="#52514e", length=0)
    for _side in ("top", "right", "left"):
        _ax.spines[_side].set_visible(False)
    _ax.spines["bottom"].set_color("#c9c8c2")
    _ax.legend(loc="upper left", frameon=False, ncols=len(_modes))
    _ax.set_xlabel("poll (UTC)", color="#52514e")
    _ax.xaxis.set_major_formatter(
        mdates.DateFormatter("%d %b %H:%M", tz="UTC")
    )
    _fig.tight_layout()
    _fig
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## How often trains run on each segment

    A segment is a mode with a departure stop and an arrival stop. `segment` counts the distinct
    trains seen on each over the capture, and keeps one of their shapes. Two platforms of one station
    are two stops, so segments between them overlap on the map.
    """)
    return


@app.cell
def _(CAPTURE, bronze):
    SEGMENT = CAPTURE + (
        """
    segment AS (
        SELECT mode, from_stop_id, to_stop_id,
               count(DISTINCT trip_id) AS trains,
               array_to_string(array_agg(DISTINCT route_name), ', ') AS routes,
               first_value(polyline) AS polyline
        FROM capture
        GROUP BY mode, from_stop_id, to_stop_id
    )""",
    )
    bronze(
        """
        SELECT mode, count(*) AS segments,
               min(trains) AS min_trains,
               approx_percentile_cont(trains, 0.5) AS median_trains,
               approx_percentile_cont(trains, 0.9) AS p90_trains,
               max(trains) AS max_trains
        FROM segment GROUP BY mode ORDER BY mode
        """,
        SEGMENT,
    )
    return (SEGMENT,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Every segment, by trains seen

    Colour runs from yellow, for a segment one train used, to dark red, for the busiest, on a log
    scale. Hover over a segment for its stops, its routes, and its count.
    """)
    return


@app.cell
def _(
    SEGMENT,
    apply_continuous_cmap,
    bronze,
    gpd,
    lonboard,
    mcolors,
    mo,
    plt,
    shape_of,
):
    _segments = bronze(
        "SELECT mode, from_stop_id, to_stop_id, trains, routes, polyline FROM segment",
        SEGMENT,
    ).to_pandas()
    _norm = mcolors.LogNorm(vmin=1, vmax=max(_segments["trains"].max(), 2))
    _cmap = mcolors.LinearSegmentedColormap.from_list(
        "trains", plt.get_cmap("YlOrRd")([0.3, 1.0])
    )
    _layer = lonboard.PathLayer.from_geopandas(
        gpd.GeoDataFrame(
            _segments.drop(columns="polyline"),
            geometry=shape_of(_segments["polyline"]),
        ),
        get_color=apply_continuous_cmap(_norm(_segments["trains"]), _cmap),
        width_units="pixels",
        get_width=2,
    )
    _fig, _ax = plt.subplots(figsize=(6, 0.5))
    _fig.colorbar(
        plt.cm.ScalarMappable(norm=_norm, cmap=_cmap),
        cax=_ax,
        orientation="horizontal",
        label="trains seen on the segment",
    )
    mo.vstack([lonboard.Map(_layer), _fig])
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Straight shapes

    A shape the import could not route along track runs straight between its stops.
    `longest_edge_share` catches one: the length of a shape's longest straight edge, as a share of the
    shape's whole length. A shape drawn straight between its stops scores 1. A long shape along track
    scores near 0. A short leg on straight track, or one through a long tunnel, can still reach 0.5,
    since OSM draws a straight stretch as a single edge. A shape routed for part of its way and
    straight for the rest scores in between.
    """)
    return


@app.cell
def _(METRIC_CRS, gpd, np, pd, shapely):
    def longest_edge_share(shapes: gpd.GeoSeries) -> pd.Series:
        return pd.Series(
            [
                np.hypot(
                    *np.diff(shapely.get_coordinates(_line), axis=0).T
                ).max()
                / _line.length
                for _line in shapes.to_crs(METRIC_CRS)
            ],
            index=shapes.index,
        )

    return (longest_edge_share,)


@app.cell
def _(METRIC_CRS, SEGMENT, bronze, longest_edge_share, shape_of, shapely):
    _segments = bronze(
        "SELECT mode, from_stop_id, to_stop_id, trains, routes, polyline FROM segment",
        SEGMENT,
    ).to_pandas()
    _shapes = shape_of(_segments["polyline"])
    segment_straightness = _segments.assign(
        points=shapely.get_num_coordinates(_shapes),
        length_km=_shapes.to_crs(METRIC_CRS).length / 1000,
        longest_edge_share=longest_edge_share(_shapes),
    )
    segment_straightness.groupby("mode")[
        ["points", "length_km", "longest_edge_share"]
    ].describe(percentiles=[0.5, 0.95]).drop(
        columns=["mean", "std"], level=1
    ).round(2)
    return (segment_straightness,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Segments by straightness

    Segments bucketed by `longest_edge_share`, by mode. A segment of two points is drawn straight
    between its stops, and has a bucket of its own. The count axis is logarithmic, so the rare buckets
    still show.
    """)
    return


@app.cell
def _(pd, segment_straightness):
    _buckets = pd.cut(
        segment_straightness["longest_edge_share"],
        bins=[0, 0.1, 0.25, 0.5, 1],
        labels=["under 0.1", "0.1–0.25", "0.25–0.5", "0.5 and over"],
        include_lowest=True,
    ).cat.add_categories("straight, 2 points")
    _buckets[segment_straightness["points"] == 2] = "straight, 2 points"
    straightness_counts = pd.crosstab(_buckets, segment_straightness["mode"])
    _ax = straightness_counts.plot.bar(
        figsize=(11, 3.6),
        logy=True,
        rot=0,
        color=["#2a78d6", "#1baf7a", "#eb6834"][
            : straightness_counts.shape[1]
        ],
        edgecolor="white",
        xlabel="longest_edge_share",
        ylabel="segments (log scale)",
    )
    for _container in _ax.containers:
        _ax.bar_label(_container, fontsize=8, color="#52514e")
    _ax.grid(axis="y", color="#e4e3df", linewidth=0.8)
    _ax.set_axisbelow(True)
    for _side in ("top", "right", "left"):
        _ax.spines[_side].set_visible(False)
    _ax.legend(frameon=False)
    _ax.figure.tight_layout()
    _ax.figure
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### The straightest segments

    Every segment with a `longest_edge_share` of 0.5 or more, longest edge first. Select segments in
    the table to draw them. All start selected.
    """)
    return


@app.cell
def _(mo, segment_straightness):
    _straightest = (
        segment_straightness[segment_straightness["longest_edge_share"] >= 0.5]
        .sort_values("longest_edge_share", ascending=False)
        .reset_index(drop=True)
    )
    straight_table = mo.ui.table(
        _straightest,
        hidden_columns=["polyline"],
        initial_selection=list(range(len(_straightest))),
        page_size=10,
    )
    straight_table
    return (straight_table,)


@app.cell
def _(gpd, lonboard, mo, shape_of, straight_table):
    mo.stop(straight_table.value.empty, mo.md("Select a segment."))
    lonboard.Map(
        lonboard.PathLayer.from_geopandas(
            gpd.GeoDataFrame(
                straight_table.value.drop(columns="polyline"),
                geometry=shape_of(straight_table.value["polyline"]),
            ),
            get_color=[232, 123, 164],
            width_units="pixels",
            get_width=3,
        )
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Distance from track

    `longest_edge_share` reads a shape's own geometry, and cannot tell a straight line drawn because
    routing failed from straight track. A reference can. `off_track_share` is the share of a shape's
    length lying more than 20 m from any rail segment in the country's Overture extract. Each shape is
    sampled every 25 m, and each sample is tested against the rail segments.

    The 20 m tolerance follows Bast and Brosi's evaluation of pfaedle, which counts a stretch between
    stations as matched when its Fréchet distance to the true course is under 20 m. A tolerance in
    metres does not depend on how long a leg is, or how many points draw it. Straight track and
    tunnels lie on rail segments, so they score 0. A shape drawn straight between its stops leaves the
    track, and scores near 1.
    """)
    return


@app.cell
def _(bronze):
    EXTRACT = (
        """
    extract AS (
        SELECT extract_id, release FROM extract_manifest
        WHERE country = $country
        QUALIFY row_number() OVER (ORDER BY extracted_at DESC) = 1
    )""",
    )
    bronze("SELECT * FROM extract", EXTRACT)
    return (EXTRACT,)


@app.cell
def _(EXTRACT, METRIC_CRS, bronze, gpd, shapely):
    _rail = bronze(
        """
        SELECT class, geometry FROM rail_segment JOIN extract USING (extract_id)
        WHERE subtype = 'rail'
        """,
        EXTRACT,
        tables={
            "rail_segment": {"theme": "transportation", "type": "segment"}
        },
    ).to_pandas()
    rail_tracks = shapely.STRtree(
        gpd.GeoSeries.from_wkb(_rail["geometry"], crs="EPSG:4326")
        .to_crs(METRIC_CRS)
        .values
    )
    _rail["class"].value_counts().rename("rail segments")
    return (rail_tracks,)


@app.cell
def _(METRIC_CRS, gpd, np, pd, rail_tracks, shapely):
    TRACK_TOLERANCE_M = 20
    SAMPLE_STEP_M = 25

    def off_track_share(shapes: gpd.GeoSeries) -> pd.Series:
        def _share(line: shapely.LineString) -> float:
            _samples = shapely.line_interpolate_point(
                line,
                np.append(
                    np.arange(0, line.length, SAMPLE_STEP_M), line.length
                ),
            )
            _on_track = np.unique(
                rail_tracks.query(
                    _samples, predicate="dwithin", distance=TRACK_TOLERANCE_M
                )[0]
            )
            return 1 - len(_on_track) / len(_samples)

        return pd.Series(
            [_share(_line) for _line in shapes.to_crs(METRIC_CRS)],
            index=shapes.index,
        )

    return (off_track_share,)


@app.cell
def _(off_track_share, segment_straightness, shape_of):
    segment_track = segment_straightness.assign(
        off_track_share=off_track_share(
            shape_of(segment_straightness["polyline"])
        ),
    )
    segment_track.groupby("mode")["off_track_share"].describe(
        percentiles=[0.5, 0.95, 0.99]
    ).drop(columns=["mean", "std"]).round(3)
    return (segment_track,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Segments by share off track

    Segments bucketed by `off_track_share`, by mode, on a logarithmic count axis.
    """)
    return


@app.cell
def _(pd, segment_track):
    _buckets = pd.cut(
        segment_track["off_track_share"],
        bins=[-1, 0, 0.1, 0.5, 1],
        labels=["0, on track", "up to 0.1", "0.1–0.5", "over 0.5"],
    )
    _counts = pd.crosstab(_buckets, segment_track["mode"])
    _ax = _counts.plot.bar(
        figsize=(11, 3.6),
        logy=True,
        rot=0,
        color=["#2a78d6", "#1baf7a", "#eb6834"][: _counts.shape[1]],
        edgecolor="white",
        xlabel="off_track_share",
        ylabel="segments (log scale)",
    )
    for _container in _ax.containers:
        _ax.bar_label(_container, fontsize=8, color="#52514e")
    _ax.grid(axis="y", color="#e4e3df", linewidth=0.8)
    _ax.set_axisbelow(True)
    for _side in ("top", "right", "left"):
        _ax.spines[_side].set_visible(False)
    _ax.legend(frameon=False)
    _ax.figure.tight_layout()
    _ax.figure
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### The two measures, segment by segment

    One dot per segment. Many short segments on straight track score high on `longest_edge_share`,
    and every one of them scores 0 on `off_track_share`.
    """)
    return


@app.cell
def _(plt, segment_track):
    _fig, _ax = plt.subplots(figsize=(6, 4.5))
    _ax.scatter(
        segment_track["longest_edge_share"],
        segment_track["off_track_share"],
        s=24,
        color="#2a78d6",
        edgecolors="white",
        linewidths=0.8,
        zorder=3,
    )
    _ax.set_xlabel("longest_edge_share")
    _ax.set_ylabel("off_track_share")
    _ax.grid(color="#e4e3df", linewidth=0.8, zorder=0)
    for _side in ("top", "right"):
        _ax.spines[_side].set_visible(False)
    _fig.tight_layout()
    _fig
    return


if __name__ == "__main__":
    app.run()
