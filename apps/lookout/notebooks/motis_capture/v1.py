# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "duckdb==1.5.5",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "marimo==0.25.0",
#     "matplotlib==3.11.1",
#     "polyline==2.0.2",
#     "pyarrow==25.0.0",
#     "shapely==2.1.2",
# ]
# ///

import marimo

__generated_with = "0.25.1"
app = marimo.App(width="medium")


@app.cell
def _():
    from pathlib import Path

    import marimo as mo
    import duckdb
    import geopandas as gpd
    import lonboard
    import matplotlib
    import polyline
    from shapely.geometry import LineString

    return LineString, Path, duckdb, gpd, lonboard, matplotlib, mo, polyline


@app.cell
def _(mo):
    mo.md(r"""
    # Motis capture

    A check of what a Motis poller captured: one row per leg per poll, from the bronze capture
    log. It shows each poll's legs on a map, counts what each poll holds, and follows each train
    from one poll to the next.

    Train data from [Transitous](https://transitous.org/) and the
    [sources it draws on](https://transitous.org/sources/).
    """)
    return


@app.cell
def _(Path):
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
    QUERY_WINDOW_HALF = "5 minutes"
    POLYLINE_PRECISION = 5
    return MEDALLION_ROOT, POLYLINE_PRECISION, QUERY_WINDOW_HALF


@app.cell
def _(MEDALLION_ROOT, duckdb):
    con = duckdb.connect()
    con.execute("SET TimeZone = 'UTC'")
    con.execute(
        f"""
        CREATE VIEW source AS
        SELECT * FROM read_parquet('{MEDALLION_ROOT}/bronze/motis_source_v2/*.parquet')
        """
    )
    con.execute(
        f"""
        CREATE VIEW captured AS
        SELECT * FROM read_parquet(
            '{MEDALLION_ROOT}/bronze/motis_segment_v2/*/*.parquet', hive_partitioning = true
        )
        """
    )
    return (con,)


@app.cell
def _(con, mo):
    sources = con.sql(
        """
        SELECT source.*, count(captured.source_id) AS legs,
               count(DISTINCT captured.captured_at) AS polls
        FROM source LEFT JOIN captured USING (source_id)
        GROUP BY ALL
        ORDER BY registered_at
        """
    ).df()
    source_options = {
        f"{row.feed} {row.overture_region_name or row.country or 'near GPS'} {row.registered_at:%Y-%m-%d %H:%M}": row.source_id
        for row in sources.itertuples()
    }
    source_choice = mo.ui.dropdown(
        options=source_options,
        value=list(source_options)[-1],
        label="source",
    )
    mo.vstack([sources, source_choice])
    return (source_choice,)


@app.cell
def _(con, source_choice):
    con.execute(
        f"""
        CREATE OR REPLACE VIEW legs AS
        SELECT * FROM captured WHERE source_id = '{source_choice.value}'
        """
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Each poll

    `outside_window` counts the legs whose time span misses the window the poll asked about,
    centred on the poll instant. Motis answers with the legs overlapping that window, and matches
    times to the whole minute, so the window opens at the start of its minute and the count is
    zero.
    """)
    return


@app.cell
def _(QUERY_WINDOW_HALF, con, source_choice):
    source_choice.value
    con.sql(
        f"""
        SELECT captured_at AS poll,
               count(*) AS legs,
               count(DISTINCT trip_id) AS trips,
               count(DISTINCT trip_id) FILTER (WHERE first_seen = captured_at) AS new_trips,
               count(*) FILTER (WHERE realtime) AS realtime_legs,
               count(*) FILTER (WHERE train_number IS NULL) AS without_train_number,
               count(*) FILTER (WHERE agency_name IS NULL) AS without_agency,
               count(*) FILTER (
                   WHERE arrival < date_trunc('minute', captured_at - INTERVAL '{QUERY_WINDOW_HALF}')
                      OR departure > captured_at + INTERVAL '{QUERY_WINDOW_HALF}'
               ) AS outside_window
        FROM (SELECT *, min(captured_at) OVER (PARTITION BY trip_id) AS first_seen FROM legs)
        GROUP BY captured_at
        ORDER BY captured_at
        """
    ).df()
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Legs without a train number

    Grouped by route, to show whether the gap belongs to one operator or kind of train.
    """)
    return


@app.cell
def _(con, source_choice):
    source_choice.value
    con.sql(
        """
        SELECT agency_name, mode, route_name, count(*) AS legs
        FROM legs
        WHERE train_number IS NULL
        GROUP BY ALL
        ORDER BY legs DESC
        """
    ).df()
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Each poll on the map
    """)
    return


@app.cell
def _(con, matplotlib, mo, source_choice):
    source_choice.value
    polls = (
        con.sql("SELECT DISTINCT captured_at FROM legs ORDER BY captured_at")
        .df()["captured_at"]
        .tolist()
    )
    poll_colours = {
        poll: [
            round(255 * channel)
            for channel in matplotlib.colormaps["tab10"](index)[:3]
        ]
        for index, poll in enumerate(polls)
    }
    poll_choice = mo.ui.multiselect(
        options={f"{poll:%Y-%m-%d %H:%M:%S}": poll for poll in polls},
        value=[f"{poll:%Y-%m-%d %H:%M:%S}" for poll in polls],
        label="polls",
    )
    mo.hstack(
        [
            poll_choice,
            mo.md(
                "  \n".join(
                    f'<span style="color: rgb{tuple(colour)}">━━━</span> {poll:%H:%M:%S}'
                    for poll, colour in poll_colours.items()
                )
            ),
        ],
        justify="start",
    )
    return poll_choice, poll_colours


@app.cell
def _(
    LineString,
    POLYLINE_PRECISION,
    con,
    gpd,
    lonboard,
    poll_colours,
    polyline,
    source_choice,
):
    source_choice.value
    _captured = con.sql(
        """
        SELECT captured_at, trip_id, route_name, train_number, agency_name, mode, realtime,
               from_stop_id, to_stop_id, departure, arrival, polyline
        FROM legs
        """
    ).df()
    captured_legs = gpd.GeoDataFrame(
        _captured.drop(columns="polyline"),
        geometry=[
            LineString(
                polyline.decode(encoded, POLYLINE_PRECISION, geojson=True)
            )
            for encoded in _captured["polyline"]
        ],
        crs="EPSG:4326",
    )
    poll_layers = {
        poll: lonboard.PathLayer.from_geopandas(
            captured_legs[captured_legs["captured_at"] == poll],
            get_color=colour,
            width_min_pixels=2,
        )
        for poll, colour in poll_colours.items()
    }
    lonboard.Map(list(poll_layers.values()))
    return (poll_layers,)


@app.cell
def _(poll_choice, poll_layers):
    for _poll, _layer in poll_layers.items():
        _layer.visible = _poll in poll_choice.value
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Each train from poll to poll

    One row per trip per poll. A poll returns every leg of a trip that overlaps its window, so the
    row shows the leg the train was on at the poll instant (`on`). Otherwise it shows the next leg
    it would start (`next`), or the last leg it finished (`past`). `moved_on` marks a train on a
    different leg from its previous poll. `delay_change` is the change in departure delay since
    then, in minutes. Both are empty on a train's first poll.
    """)
    return


@app.cell
def _(con, source_choice):
    source_choice.value
    progress = con.sql(
        """
        WITH placed AS (
            SELECT *,
                   CASE
                       WHEN departure <= captured_at AND captured_at < arrival THEN 'on'
                       WHEN departure > captured_at THEN 'next'
                       ELSE 'past'
                   END AS status
            FROM legs
        ),
        chosen AS (
            SELECT * FROM placed
            QUALIFY row_number() OVER (
                PARTITION BY trip_id, captured_at
                ORDER BY status = 'on' DESC, status = 'next' DESC, departure DESC
            ) = 1
        )
        SELECT route_name, trip_id, captured_at AS poll, status, from_stop_id, to_stop_id,
               departure, arrival,
               date_diff('minute', scheduled_departure, departure) AS delay,
               from_stop_id <> lag(from_stop_id) OVER trip AS moved_on,
               delay - lag(delay) OVER trip AS delay_change,
               count(*) OVER (PARTITION BY trip_id) AS polls_seen
        FROM chosen
        WINDOW trip AS (PARTITION BY trip_id ORDER BY captured_at)
        ORDER BY route_name, trip_id, captured_at
        """
    ).df()
    progress
    return (progress,)


@app.cell
def _(progress):
    followed = progress[progress["polls_seen"] > 1]
    later = followed[followed.duplicated("trip_id")]
    {
        "trips": progress["trip_id"].nunique(),
        "trips seen in more than one poll": followed["trip_id"].nunique(),
        "later sightings on a new leg": int(later["moved_on"].eq(True).sum()),
        "later sightings whose delay changed": int((later["delay_change"].fillna(0) != 0).sum()),
    }
    return


if __name__ == "__main__":
    app.run()
