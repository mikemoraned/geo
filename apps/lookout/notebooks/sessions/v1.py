# /// script
# dependencies = [
#     "duckdb==1.5.5",
#     "folium==0.20.0",
#     "geopandas==1.1.4",
#     "mapclassify==2.10.0",
#     "marimo",
#     "matplotlib==3.11.1",
# ]
# requires-python = ">=3.13"
# ///

import marimo

__generated_with = "0.23.15"
app = marimo.App(width="medium")


@app.cell
def _():
    from pathlib import Path

    import marimo as mo
    import duckdb
    import geopandas as gpd

    return Path, duckdb, gpd, mo


@app.cell
def _(Path):
    # The medallion store `just sessionise` writes into: the one in the repo, found by walking
    # up for the workspace the way the Rust CLIs do, so this notebook and they always read the
    # same store however deep the working directory is.
    MEDALLION_ROOT = next(
        parent / "data/medallion"
        for parent in Path.cwd().resolve().parents
        if (parent / "Cargo.toml").is_file()
        and "[workspace]" in (parent / "Cargo.toml").read_text()
    )

    # The CRS the store's projected geometry is in: one zone per country, and everything
    # recorded so far is in Germany. Distances and buffers in metres come from this column.
    PROJECTED_CRS = "EPSG:25832"

    # The CRS the store's lat/lon geometry is in.
    CRS84 = "EPSG:4326"
    return CRS84, MEDALLION_ROOT, PROJECTED_CRS


@app.cell
def _(duckdb):
    con = duckdb.connect()
    # Spatial reads a GeoParquet geometry column as geometry rather than as a WKB blob, and
    # gives the ST_* functions the reads below select through.
    con.execute("INSTALL spatial; LOAD spatial;")
    # The store keeps every instant in UTC and partitions by the UTC date. Left to itself
    # DuckDB renders instants in the machine's zone, which would show a session on a different
    # day from the partition holding it, so this reads the store in the store's own clock.
    con.execute("SET TimeZone = 'UTC'")
    return (con,)


@app.cell
def _(CRS84, MEDALLION_ROOT, PROJECTED_CRS, con, gpd):
    def read_silver(
        dataset: str, columns: str, order_by: str
    ) -> gpd.GeoDataFrame:
        """One silver dataset as a GeoDataFrame in lat/lon.

        `columns` are selected as they are; the lat/lon `geometry` column comes back as WKB and
        becomes the frame's geometry, and `geometry_projected` rides along as `projected` for
        the work that has to be in metres. Hive partitioning is on, so `country` and the date
        key are columns a predicate could prune whole files by.
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
        """session_id, device_id, started_at, ended_at, sample_count, started_by, gap_seconds,
           CAST(start_date AS VARCHAR) AS day""",
        "started_at",
    )
    samples = read_silver(
        "session_sample",
        "session_id, device_id, t, seq, acc, speed, heading, implied_speed_mps",
        "session_id, seq",
    )

    mo.md(
        f"**{len(sessions)}** sessions, **{len(samples)}** samples, "
        f"**{sessions['device_id'].nunique()}** devices, "
        f"**{sessions['day'].nunique()}** days."
    )
    return samples, sessions


@app.cell
def _(mo, sessions):
    # What everything below is limited to. The day is the UTC date a session started on,
    # which is also the partition it lives in; the device is the leading characters of its id,
    # since a full uuid is unreadable in a dropdown and six characters separate the devices
    # that have recorded so far. Either filter set to its `ALL_` option stands aside.
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
    mo.hstack([day, device], justify="start", gap=2)
    return ALL_DAYS, ALL_DEVICES, day, device


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
def _(chosen_sessions, mo, show):
    # Every chosen path at once, one colour per session from a matplotlib colormap: the shape
    # of what was recorded, and where the split between one session and the next falls. No
    # legend — a day can hold enough sessions to bury the map — so the session is named in the
    # tooltip.
    mo.md("No sessions match.") if chosen_sessions.empty else show(
        chosen_sessions.drop(columns=["projected"]).explore(
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
            style_kwds={"weight": 3},
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
    # The selected session's samples, each as the circle its reported accuracy claims: the
    # buffer is taken on the projected geometry, so the radius is metres to scale, and the
    # result is drawn back in lat/lon.
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
