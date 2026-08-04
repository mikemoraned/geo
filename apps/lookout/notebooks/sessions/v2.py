# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "duckdb==1.5.5",
#     "folium==0.20.0",
#     "geopandas==1.1.4",
#     "mapclassify==2.10.0",
#     "marimo>=0.23.16",
#     "matplotlib==3.11.1",
# ]
# ///

import marimo

__generated_with = "0.23.15"
app = marimo.App()


@app.cell
def _():
    from pathlib import Path

    import marimo as mo
    import duckdb
    import geopandas as gpd

    return Path, duckdb, gpd, mo


@app.cell
def _(Path):
    # The medallion store the CLIs write into: the one in the repo, found by walking up for the
    # workspace the way they do — the starting directory included, since that is where the
    # workspace manifest sits — so this notebook always reads the store they last wrote.
    def medallion_root() -> Path:
        start = Path.cwd().resolve()
        for directory in (start, *start.parents):
            manifest = directory / "Cargo.toml"
            if manifest.is_file() and "[workspace]" in manifest.read_text():
                return directory / "data/medallion"
        raise FileNotFoundError(
            f"no Cargo.toml declaring [workspace] at or above {start}, "
            f"so the store's location cannot be worked out"
        )

    MEDALLION_ROOT = medallion_root()

    # The CRS the store's projected geometry is in: one zone per country, and everything recorded
    # so far is in Germany. Distances in metres are measured on that column.
    PROJECTED_CRS = "EPSG:25832"

    # The CRS the store's lat/lon geometry is in.
    CRS84 = "EPSG:4326"
    return CRS84, MEDALLION_ROOT, PROJECTED_CRS


@app.cell
def _(duckdb):
    con = duckdb.connect()
    # Spatial reads a GeoParquet geometry column as geometry rather than as a WKB blob.
    con.execute("INSTALL spatial; LOAD spatial;")
    # The store keeps every instant in UTC and partitions by the UTC date; reading it in the
    # machine's zone would put a row on a different day from the partition holding it.
    con.execute("SET TimeZone = 'UTC'")
    return (con,)


@app.cell
def _(CRS84, MEDALLION_ROOT, PROJECTED_CRS, con, gpd):
    def read_silver(
        dataset: str, columns: str, order_by: str
    ) -> gpd.GeoDataFrame:
        """One silver dataset as a GeoDataFrame in lat/lon.

        `columns` are selected as they are; the lat/lon `geometry` column becomes the frame's
        geometry and `geometry_projected` rides along as `projected`, for the work that has to be
        in metres. Hive partitioning is on, so the partition keys are columns like any other.
        """
        frame = con.execute(
            f"""
            SELECT {columns},
                   ST_AsWKB(geometry) AS wkb,
                   ST_AsWKB(geometry_projected) AS wkb_projected
            FROM read_parquet('{MEDALLION_ROOT}/silver/{dataset}/**/*.parquet',
                              hive_partitioning = 1)
            ORDER BY {order_by}
            """
        ).df()

        # DuckDB hands back a bytearray per row; shapely reads bytes.
        geometry = gpd.GeoSeries.from_wkb(frame["wkb"].map(bytes), crs=CRS84)
        projected = gpd.GeoSeries.from_wkb(
            frame["wkb_projected"].map(bytes), crs=PROJECTED_CRS
        )
        return gpd.GeoDataFrame(
            frame.drop(columns=["wkb", "wkb_projected"]).assign(
                projected=projected
            ),
            geometry=geometry,
            crs=CRS84,
        )

    return (read_silver,)


@app.cell
def _(mo):
    def show(explored, height: int = 500):
        """A folium map as marimo output.

        `GeoDataFrame.explore` returns a folium map whose `_repr_html_` is an `srcdoc` iframe
        behind a "Make this Notebook Trusted" fallback — a Jupyter notion with no counterpart
        here, and the iframe does not survive sanitising. Rendering the map to standalone HTML
        and hosting it in marimo's own iframe shows the map itself.
        """
        return mo.iframe(explored.get_root().render(), height=height)

    return (show,)


@app.cell
def _(mo, read_silver):
    sessions = read_silver(
        "session",
        """session_id, device_id, started_at, ended_at, sample_count, started_by,
           CAST(start_date AS VARCHAR) AS day""",
        "started_at",
    )
    samples = read_silver(
        "session_sample",
        "session_id, device_id, t, seq, acc, speed, implied_speed_mps",
        "session_id, seq",
    )
    legs = read_silver(
        "train_segment",
        """trip_id, route_name, train_number, mode, realtime, departure, arrival,
           CAST(departure_date AS VARCHAR) AS day""",
        "departure",
    )

    mo.md(
        f"**{len(sessions)}** sessions and **{len(samples)}** samples over "
        f"**{sessions['day'].nunique()}** days; **{len(legs)}** train legs over "
        f"**{legs['day'].nunique()}** days."
    )
    return legs, samples, sessions


@app.cell
def _(mo, sessions):
    # What everything below is limited to. The day is the UTC date a session started on, which
    # is also the partition it lives in; the device is the leading characters of its id, since a
    # full uuid is unreadable in a dropdown. Either filter set to its `ALL_` option stands aside.
    # `near` is what counts as a train running alongside a trace, in metres.
    ALL_DAYS = "all days"
    ALL_DEVICES = "all devices"
    DEVICE_PREFIX = 6

    day = mo.ui.dropdown(
        options=[ALL_DAYS, *sorted(sessions["day"].unique())],
        value=ALL_DAYS,
        label="Day",
    )
    device = mo.ui.dropdown(
        options=[
            ALL_DEVICES,
            *sorted(sessions["device_id"].str[:DEVICE_PREFIX].unique()),
        ],
        value=ALL_DEVICES,
        label="Device",
    )
    near = mo.ui.slider(
        steps=[100, 250, 500, 1000, 2500, 5000, 10000],
        value=500,
        label="Trains within (m)",
        show_value=True,
    )
    mo.hstack([day, device, near], justify="start", gap=2)
    return ALL_DAYS, ALL_DEVICES, day, device, near


@app.cell
def _(ALL_DAYS, ALL_DEVICES, day, device, sessions):
    # Each filter narrows in turn, so either one left at its `ALL_` option simply does not
    # narrow — rather than being expressed as a mask that has to cover the whole frame.
    _chosen = sessions
    if day.value != ALL_DAYS:
        _chosen = _chosen[_chosen["day"] == day.value]
    if device.value != ALL_DEVICES:
        _chosen = _chosen[_chosen["device_id"].str.startswith(device.value)]

    chosen_sessions = _chosen
    return (chosen_sessions,)


@app.cell
def _(chosen_sessions, gpd, legs, mo, near):
    # The trains worth drawing: those recorded the same day as a session on the map, and running
    # within `near` metres of one. A day holds hundreds of legs and a trace runs along a handful
    # of lines, so without this the trains bury what they are meant to be compared against.
    #
    # The same day means the same partition date — a leg's `departure_date` against a session's
    # `start_date`. That is the simple reading rather than the exact one, since a session starting
    # at 23:50 and a leg departing at 00:10 are one journey to a reader and two dates to the store.
    #
    # Distance is metric, so the join is on the projected geometry both datasets carry. A leg near
    # several sessions comes back once, at its shortest distance.
    #
    # Legs are then collapsed onto the track they run along: a stretch of line carries a leg per
    # trip that used it, and drawing the same geometry eighty times costs eighty times as much
    # while saying nothing more. The nearest leg of each stretch is kept and `legs_here` records
    # how many shared it, so the count is still on the map rather than lost to the collapse.
    _same_day = legs[legs["day"].isin(chosen_sessions["day"])]

    if _same_day.empty or chosen_sessions.empty:
        nearby_legs = legs.iloc[:0].assign(metres_away=0.0, legs_here=0)
    else:
        _matched = gpd.sjoin_nearest(
            _same_day.set_geometry("projected"),
            chosen_sessions.set_geometry("projected")[
                ["session_id", "projected"]
            ],
            max_distance=near.value,
            distance_col="metres_away",
            how="inner",
        ).sort_values("metres_away")
        _once = _matched[~_matched.index.duplicated()]
        _near = _same_day.loc[_once.index].assign(
            metres_away=_once["metres_away"],
            track=_same_day.loc[_once.index].geometry.to_wkb(),
        )
        _per_track = _near.groupby("track").size()
        nearby_legs = (
            _near.sort_values("metres_away")
            .drop_duplicates(subset="track")
            .assign(legs_here=lambda frame: frame["track"].map(_per_track))
            .drop(columns="track")
        )

    mo.md(
        f"**{len(chosen_sessions)}** sessions, and **{nearby_legs['legs_here'].sum()}** train legs "
        f"within **{near.value} m** of one of them — on **{len(nearby_legs)}** stretches of track — "
        f"of **{len(_same_day)}** legs recorded the same day."
    )
    return (nearby_legs,)


@app.cell
def _(chosen_sessions, mo):
    # Pick a session to see its samples below.
    session_table = mo.ui.table(
        chosen_sessions.drop(columns=["geometry", "projected"]),
        selection="single",
        page_size=10,
    )
    session_table
    return (session_table,)


@app.cell
def _(chosen_sessions, mo, nearby_legs, show):
    # The trains and the traces on one map: the track thin and muted, one colour per session over
    # the top, since the question being asked is which of these lines a trace ran along.
    _map = (
        nearby_legs.drop(columns=["projected"]).explore(
            color="#6b7280",
            tiles="CartoDB positron",
            tooltip=[
                "route_name",
                "train_number",
                "mode",
                "departure",
                "metres_away",
                "legs_here",
            ],
            style_kwds={"weight": 1, "opacity": 0.6},
        )
        if not nearby_legs.empty
        else None
    )

    mo.md("No sessions match.") if chosen_sessions.empty else show(
        chosen_sessions.drop(columns=["projected"]).explore(
            m=_map,
            column="session_id",
            categorical=True,
            cmap="turbo",
            legend=False,
            tiles="CartoDB positron",
            tooltip=[
                "session_id",
                "device_id",
                "started_at",
                "sample_count",
                "started_by",
            ],
            style_kwds={"weight": 4},
        )
    )
    return


@app.cell
def _(samples, session_table):
    selected = (
        samples[samples["session_id"].isin(session_table.value["session_id"])]
        if len(session_table.value)
        else samples.iloc[:0]
    )
    return (selected,)


@app.cell
def _(CRS84, mo, selected, show):
    # The selected session's samples, each as the circle its reported accuracy claims: the buffer
    # is taken on the projected geometry, so the radius is metres to scale, and the result is
    # drawn back in lat/lon.
    accuracy_circles = selected.set_geometry(
        selected["projected"].buffer(selected["acc"])
    ).to_crs(CRS84)

    mo.md(
        "Select a session above to see its samples."
    ) if selected.empty else show(
        accuracy_circles.drop(columns=["projected"]).explore(
            column="acc",
            cmap="viridis",
            legend=True,
            tiles="CartoDB positron",
            tooltip=["seq", "t", "acc", "speed", "implied_speed_mps"],
        )
    )
    return


if __name__ == "__main__":
    app.run()
