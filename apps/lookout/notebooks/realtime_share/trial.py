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
    from datetime import UTC, datetime

    import marimo as mo
    import altair as alt
    import geopandas as gpd
    import lonboard
    import numpy as np
    import polyline
    import pyarrow as pa
    import shapely

    import lookout_medallion

    return (
        UTC,
        alt,
        datetime,
        gpd,
        lonboard,
        lookout_medallion,
        mo,
        np,
        pa,
        polyline,
        shapely,
    )


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
def _(UTC, datetime, mo):
    refresh = mo.ui.button(
        value=datetime.now(UTC),
        on_click=lambda _: datetime.now(UTC),
        label="Refresh polls",
    )
    refresh
    return (refresh,)


@app.cell
def _(lookout_medallion, mo, pa, refresh, source_id):
    def poll_query(sql):
        return pa.table(
            lookout_medallion.query_landing(
                sql, params={"source_id": source_id}
            )
        )

    mo.md(f"Polls landed by {refresh.value:%Y-%m-%d %H:%M:%S} UTC")
    return (poll_query,)


@app.cell
def _(poll_query):
    legs = poll_query(
        """
        SELECT captured_at, trip_id, route_name, agency_name, mode,
               from_stop_id, from_lat, from_lon, to_stop_id, to_lat, to_lon,
               departure, arrival, scheduled_departure, scheduled_arrival, realtime
        FROM motis_poll_v2
        WHERE source_id = $source_id
        """
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
def _(alt, mo, poll_query):
    share_by_mode = poll_query(
        """
        SELECT captured_at, mode, count(*) AS legs,
               sum(CAST(realtime AS INT)) AS realtime,
               avg(CAST(realtime AS DOUBLE)) AS share
        FROM motis_poll_v2
        WHERE source_id = $source_id
        GROUP BY captured_at, mode
        ORDER BY captured_at, mode
        """
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
def _(mo, poll_query):
    share_by_poll = poll_query(
        """
        SELECT captured_at, count(*) AS legs,
               sum(CAST(realtime AS INT)) AS realtime,
               avg(CAST(realtime AS DOUBLE)) AS share,
               count(DISTINCT trip_id) AS trips,
               count(DISTINCT CASE WHEN NOT realtime THEN trip_id END) AS unmarked_trips
        FROM motis_poll_v2
        WHERE source_id = $source_id
        GROUP BY captured_at
        ORDER BY captured_at
        """
    ).to_pandas()
    mo.vstack(
        [
            share_by_poll[["share", "unmarked_trips"]].describe().T,
            share_by_poll,
        ]
    )
    return


@app.cell
def _(alt, poll_query):
    trips_by_mode = poll_query(
        """
        SELECT captured_at, mode, count(DISTINCT trip_id) AS trips
        FROM motis_poll_v2
        WHERE source_id = $source_id
        GROUP BY captured_at, mode
        ORDER BY captured_at, mode
        """
    ).to_pandas()
    alt.Chart(trips_by_mode).mark_area().encode(
        x=alt.X("captured_at:T", title="poll (UTC)"),
        y=alt.Y("trips:Q", title="trips", stack="zero"),
        color=alt.Color("mode:N"),
        tooltip=["captured_at:T", "mode:N", "trips:Q"],
    ).properties(width=640, height=240)
    return


@app.cell
def _(poll_query):
    unmarked = poll_query(
        """
        WITH ever_unmarked AS (
            SELECT DISTINCT trip_id
            FROM motis_poll_v2
            WHERE source_id = $source_id AND NOT realtime
        )
        SELECT p.trip_id, p.agency_name, p.mode,
               count(DISTINCT CASE WHEN p.realtime THEN p.captured_at END) AS polls_marked,
               count(DISTINCT CASE WHEN NOT p.realtime THEN p.captured_at END) AS polls_unmarked,
               min(CASE WHEN NOT p.realtime THEN p.captured_at END) AS first_unmarked,
               max(CASE WHEN NOT p.realtime THEN p.captured_at END) AS last_unmarked,
               min(p.departure) AS first_departure,
               max(p.arrival) AS last_arrival
        FROM motis_poll_v2 p
        JOIN ever_unmarked USING (trip_id)
        WHERE p.source_id = $source_id
        GROUP BY p.trip_id, p.agency_name, p.mode
        ORDER BY first_unmarked, p.trip_id
        """
    ).to_pandas()
    unmarked
    return (unmarked,)


@app.cell
def _(poll_query):
    night_trips = poll_query(
        """
        SELECT trip_id,
               count(DISTINCT CASE WHEN realtime THEN captured_at END) AS polls_marked,
               count(DISTINCT CASE WHEN NOT realtime THEN captured_at END) AS polls_unmarked,
               min(departure) AS first_departure,
               max(arrival) AS last_arrival
        FROM motis_poll_v2
        WHERE source_id = $source_id AND mode = 'NIGHT_RAIL'
        GROUP BY trip_id
        ORDER BY trip_id
        """
    ).to_pandas()
    night_trips
    return


@app.cell
def _(gpd, poll_query, polyline, shapely):
    traces = gpd.GeoDataFrame(
        poll_query(
            """
            SELECT captured_at, trip_id, mode, agency_name, route_name,
                   departure, arrival, realtime, polyline
            FROM motis_poll_v2
            WHERE source_id = $source_id
            ORDER BY captured_at, trip_id
            """
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


@app.cell
def _(lonboard, mo, np, traces, unmarked):
    _unmarked_traces = traces[
        traces.trip_id.isin(unmarked.trip_id) & ~traces.realtime
    ]
    _trip_colours = {
        trip_id: colour
        for trip_id, colour in zip(
            unmarked.trip_id,
            [
                [220, 40, 40],
                [240, 140, 0],
                [150, 60, 200],
                [0, 150, 140],
                [200, 0, 120],
                [90, 90, 90],
            ],
        )
    }
    mo.vstack(
        [
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        traces[traces.realtime][["geometry"]],
                        get_color=[200, 200, 200],
                        width_min_pixels=1,
                    ),
                    lonboard.PathLayer.from_geopandas(
                        _unmarked_traces[
                            ["trip_id", "agency_name", "geometry"]
                        ],
                        get_color=np.array(
                            [
                                _trip_colours.get(trip_id, [90, 90, 90])
                                for trip_id in _unmarked_traces.trip_id
                            ],
                            dtype=np.uint8,
                        ),
                        width_min_pixels=4,
                    ),
                ],
                height=560,
            ),
            unmarked[["agency_name", "trip_id"]].assign(
                colour=[
                    str(_trip_colours.get(trip_id))
                    for trip_id in unmarked.trip_id
                ]
            ),
        ]
    )
    return


if __name__ == "__main__":
    app.run()
