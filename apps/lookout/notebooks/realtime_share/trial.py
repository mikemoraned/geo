# /// script
# dependencies = [
#     "altair==6.3.0",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo>=0.25.1",
#     "numpy==2.5.5",
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

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import altair as alt
    import geopandas as gpd
    import lonboard
    import numpy as np
    import polyline
    import pyarrow as pa
    import shapely

    import lookout_medallion

    return alt, gpd, lonboard, lookout_medallion, mo, np, pa, polyline, shapely


@app.cell
def _(lookout_medallion, pa):
    label = "aubin-2026-11-18.route-shapes"
    source = pa.table(
        lookout_medallion.query_bronze(
            "SELECT source_id, base_url, motis_version, feed, country, overture_region_name, label, registered_at FROM motis_source_v4 WHERE label = $label",
            params={"label": label},
        )
    )
    assert source.num_rows == 1, (
        f"{source.num_rows} sources carry the label {label}"
    )
    source_id = source.column("source_id")[0].as_py()
    source.to_pandas()
    return (source_id,)


@app.cell
def _(lookout_medallion, pa, source_id):
    legs = pa.table(
        lookout_medallion.query_landing(
            """
            SELECT captured_at, trip_id, route_name, agency_name, mode,
                   from_stop_id, from_lat, from_lon, to_stop_id, to_lat, to_lon,
                   departure, arrival, scheduled_departure, scheduled_arrival, realtime
            FROM motis_poll_v2
            WHERE source_id = $source_id
            """,
            params={"source_id": source_id},
        )
    )
    (
        legs.group_by("captured_at")
        .aggregate([("trip_id", "count"), ("realtime", "sum")])
        .rename_columns(["captured_at", "legs", "realtime"])
        .sort_by("captured_at")
        .to_pandas()
    )
    return


@app.cell
def _(alt, lookout_medallion, mo, pa, source_id):
    share_by_mode = pa.table(
        lookout_medallion.query_landing(
            """
            SELECT captured_at, mode, count(*) AS legs,
                   sum(CAST(realtime AS INT)) AS realtime,
                   avg(CAST(realtime AS DOUBLE)) AS share
            FROM motis_poll_v2
            WHERE source_id = $source_id
            GROUP BY captured_at, mode
            ORDER BY captured_at, mode
            """,
            params={"source_id": source_id},
        )
    ).to_pandas()
    mo.vstack(
        [
            alt.Chart(share_by_mode)
            .mark_line(point=True)
            .encode(
                x=alt.X("captured_at:T", title="poll (UTC)"),
                y=alt.Y(
                    "share:Q",
                    title="share of legs marked realtime",
                    scale=alt.Scale(domain=[0, 1]),
                ),
                color=alt.Color("mode:N"),
                tooltip=[
                    "captured_at:T",
                    "mode:N",
                    "legs:Q",
                    "realtime:Q",
                    alt.Tooltip("share:Q", format=".3f"),
                ],
            )
            .properties(width=640, height=240),
            share_by_mode,
        ]
    )
    return


@app.cell
def _(alt, lookout_medallion, pa, source_id):
    trips_by_mode = pa.table(
        lookout_medallion.query_landing(
            """
            SELECT captured_at, mode, count(DISTINCT trip_id) AS trips
            FROM motis_poll_v2
            WHERE source_id = $source_id
            GROUP BY captured_at, mode
            ORDER BY captured_at, mode
            """,
            params={"source_id": source_id},
        )
    ).to_pandas()
    alt.Chart(trips_by_mode).mark_area().encode(
        x=alt.X("captured_at:T", title="poll (UTC)"),
        y=alt.Y("trips:Q", title="trips", stack="zero"),
        color=alt.Color("mode:N"),
        tooltip=["captured_at:T", "mode:N", "trips:Q"],
    ).properties(width=640, height=240)
    return


@app.cell
def _(lookout_medallion, pa, source_id):
    unmarked = pa.table(
        lookout_medallion.query_landing(
            """
            SELECT mode, agency_name, route_name,
                   count(*) AS legs,
                   count(DISTINCT trip_id) AS trips,
                   count(DISTINCT captured_at) AS polls,
                   min(captured_at) AS first_poll,
                   max(captured_at) AS last_poll
            FROM motis_poll_v2
            WHERE source_id = $source_id AND NOT realtime
            GROUP BY mode, agency_name, route_name
            ORDER BY legs DESC
            """,
            params={"source_id": source_id},
        )
    ).to_pandas()
    unmarked
    return


@app.cell
def _(gpd, lookout_medallion, pa, polyline, shapely, source_id):
    traces = gpd.GeoDataFrame(
        pa.table(
            lookout_medallion.query_landing(
                """
                SELECT captured_at, trip_id, mode, agency_name, route_name,
                       departure, arrival, realtime, polyline
                FROM motis_poll_v2
                WHERE source_id = $source_id
                ORDER BY captured_at, trip_id
                """,
                params={"source_id": source_id},
            )
        )
        .to_pandas()
        .assign(
            geometry=lambda frame: [
                shapely.LineString(
                    [(lon, lat) for lat, lon in polyline.decode(text, 5)]
                )
                for text in frame.polyline
            ]
        )
        .drop(columns="polyline"),
        crs="EPSG:4326",
    )
    traces.geometry.apply(lambda line: len(line.coords)).describe().to_frame(
        "points per trace"
    )
    return (traces,)


@app.cell
def _(mo, traces):
    polls = {
        f"{at:%Y-%m-%d %H:%M}": at
        for at in sorted(traces.captured_at.unique())
    }
    poll = mo.ui.dropdown(
        options=polls, value=list(polls)[-1], label="poll (UTC)"
    )
    poll
    return (poll,)


@app.cell
def _(mo, poll, traces):
    poll_traces = (
        traces[traces.captured_at == poll.value]
        .sort_values(["realtime", "agency_name", "route_name", "departure"])
        .reset_index(drop=True)
    )
    leg_table = mo.ui.table(
        poll_traces[
            [
                "trip_id",
                "agency_name",
                "route_name",
                "mode",
                "realtime",
                "departure",
                "arrival",
            ]
        ],
        selection="multi",
        page_size=15,
        label=f"{len(poll_traces)} legs, {(~poll_traces.realtime).sum()} without realtime",
    )
    leg_table
    return leg_table, poll_traces


@app.cell
def _(leg_table, lonboard, np, poll_traces):
    _realtime_blue = np.array([30, 110, 220], dtype=np.uint8)
    _unmarked_red = np.array([220, 40, 40], dtype=np.uint8)
    _selected = poll_traces.loc[leg_table.value.index]
    _layers = [
        lonboard.PathLayer.from_geopandas(
            poll_traces[["realtime", "geometry"]],
            get_color=np.where(
                poll_traces.realtime.to_numpy()[:, None],
                _realtime_blue,
                _unmarked_red,
            ),
            width_min_pixels=2,
        )
    ]
    if len(_selected):
        _layers.append(
            lonboard.PathLayer.from_geopandas(
                _selected[["geometry"]],
                get_color=[250, 200, 0],
                width_min_pixels=6,
            )
        )
    lonboard.Map(_layers, height=560)
    return


if __name__ == "__main__":
    app.run()
