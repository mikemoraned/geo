# /// script
# dependencies = [
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo",
#     "matplotlib==3.11.1",
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
    import pandas as pd
    import polyline
    import pyarrow as pa
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
        pa,
        pd,
        plt,
        polyline,
    )


@app.cell
def _(mo):
    mo.md(r"""
    # Germany: `delfi-2026-10-10.route-shapes`

    Every mainline and regional train in Germany, captured from the local Motis server every 10
    minutes over a day. The import computed rail shapes along OSM track with `route_shapes`, from the
    DELFI build of 10 October 2026.

    Each step below adds one named query to those before it, and shows what it returns.
    """)
    return


@app.cell
def _(LineString, gpd, lookout_medallion, pa, pd, polyline):
    MEDALLION_ROOT = lookout_medallion.default_root()
    LABEL = "delfi-2026-10-10.route-shapes"
    COUNTRY = "DE"
    POLYLINE_PRECISION = 5
    # Motis encodes each shape as a Google polyline, whose points are WGS 84 latitude and longitude.
    POLYLINE_CRS = "EPSG:4326"

    def bronze(sql: str, steps: tuple[str, ...] = ()) -> pa.Table:
        if steps:
            sql = "WITH " + ",\n".join(steps) + "\n" + sql
        return pa.table(
            lookout_medallion.query_bronze(
                sql,
                params=dict(label=LABEL, country=COUNTRY),
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

    return bronze, shape_of


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


if __name__ == "__main__":
    app.run()
