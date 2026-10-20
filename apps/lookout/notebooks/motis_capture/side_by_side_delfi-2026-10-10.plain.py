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
    import matplotlib.pyplot as plt
    import pandas as pd
    import polyline
    import pyarrow as pa
    from shapely.geometry import LineString

    import lookout_medallion

    return (
        LineString,
        gpd,
        lonboard,
        lookout_medallion,
        mo,
        pa,
        pd,
        plt,
        polyline,
    )


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # Side by side

    The local Motis server against Transitous, over one pass. Both servers were polled for every train
    in Thuringia, every 10 minutes, from the same DELFI build. A leg from one server joins a leg from
    the other on DELFI's trip id, the departure stop, and the scheduled departure, within the same
    poll. Each server prefixes these ids with its own dataset name, and the join strips the prefix.

    Train data from [Transitous](https://transitous.org/) and the
    [sources it draws on](https://transitous.org/sources/).
    """)
    return


@app.cell
def _(LineString, gpd, lonboard, lookout_medallion, pa, pd, polyline):
    MEDALLION_ROOT = lookout_medallion.default_root()
    LOCAL_LABEL = "delfi-2026-10-10.plain"
    DELFI_TRIP = r"^.*_(de-DELFI|germanygtfs)_"
    DELFI_STOP = r"^(de-DELFI|germanygtfs)_"
    POLYLINE_PRECISION = 5
    # Motis encodes each shape as a Google polyline, whose points are WGS 84 latitude and longitude.
    POLYLINE_CRS = "EPSG:4326"
    # Shape distances need a CRS in metres. The store projects German data to ETRS89 / UTM zone 32N.
    METRIC_CRS = lookout_medallion.projected_crs("DE")
    SERVER_COLOUR = {"local": "#1baf7a", "transitous": "#e87ba4"}

    def bronze(sql: str, steps: tuple[str, ...] = ()) -> pa.Table:
        if steps:
            sql = "WITH " + ",\n".join(steps) + "\n" + sql
        return pa.table(
            lookout_medallion.query_bronze(
                sql,
                params=dict(
                    label=LOCAL_LABEL,
                    delfi_trip=DELFI_TRIP,
                    delfi_stop=DELFI_STOP,
                ),
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

    def server_map(
        legs: pd.DataFrame, encoded: str = "polyline"
    ) -> lonboard.Map:
        _legs = gpd.GeoDataFrame(
            legs.drop(columns=legs.filter(like="polyline").columns).astype(
                {
                    column: str
                    for column in legs.select_dtypes(
                        ["datetimetz", "datetime"]
                    )
                }
            ),
            geometry=shape_of(legs[encoded]),
        )
        return lonboard.Map(
            [
                lonboard.PathLayer.from_geopandas(
                    _legs[_legs["server"] == server],
                    get_color=[
                        int(colour[index : index + 2], 16)
                        for index in (1, 3, 5)
                    ],
                    width_units="pixels",
                    get_width=3,
                )
                for server, colour in SERVER_COLOUR.items()
                if (_legs["server"] == server).any()
            ]
        )

    return METRIC_CRS, SERVER_COLOUR, bronze, server_map, shape_of


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## The pass

    The local source is the one carrying the pass's label. The Transitous source is the one that
    polled the same region over the same span. Both pollers started together, so a poll from each
    lands within a second of the other, and rounding each instant to the minute pairs them.

    Each step below adds one named query to those before it and shows what that query returns.
    """)
    return


@app.cell
def _(bronze):
    LOCAL_SOURCE = (
        """
    local_source AS (
        SELECT source_id, overture_region_id FROM motis_source_v2 WHERE feed = 'local' AND label = $label
    )""",
    )
    bronze("SELECT * FROM local_source", LOCAL_SOURCE)
    return (LOCAL_SOURCE,)


@app.cell
def _(LOCAL_SOURCE, bronze):
    SPAN = LOCAL_SOURCE + (
        """
    span AS (
        SELECT min(captured_at) AS first_poll, max(captured_at) AS last_poll
        FROM motis_segment_v2 WHERE source_id IN (SELECT source_id FROM local_source)
    )""",
    )
    bronze("SELECT * FROM span", SPAN)
    return (SPAN,)


@app.cell
def _(mo):
    mo.md(r"""
    Every Transitous source for the region is a candidate. Only the one that polled within the span
    contributes legs, at the next step.
    """)
    return


@app.cell
def _(SPAN, bronze):
    PASS_SOURCE = SPAN + (
        """
    pass_source AS (
        SELECT source_id, 'local' AS server FROM local_source
        UNION ALL
        SELECT source_id, 'transitous' AS server FROM motis_source_v2
        WHERE feed = 'transitous' AND overture_region_id IN (SELECT overture_region_id FROM local_source)
    )""",
    )
    bronze(
        """
        SELECT server, source_id, base_url, overture_release, registered_at
        FROM pass_source JOIN motis_source_v2 USING (source_id)
        ORDER BY server, registered_at
        """,
        PASS_SOURCE,
    )
    return (PASS_SOURCE,)


@app.cell
def _(mo):
    mo.md(r"""
    `pass_capture` holds every row captured within a minute of the span, with its server and its
    poll. A source with no rows in the span is absent from the count.
    """)
    return


@app.cell
def _(PASS_SOURCE, bronze):
    PASS_CAPTURE = PASS_SOURCE + (
        """
    pass_capture AS (
        SELECT
            server, source_id,
            date_trunc('minute', captured_at + INTERVAL '30 seconds') AS poll,
            trip_id, route_name, mode,
            from_stop_id, from_lat, from_lon, to_stop_id, to_lat, to_lon,
            scheduled_departure, scheduled_arrival, departure, arrival, realtime, polyline
        FROM motis_segment_v2 JOIN pass_source USING (source_id), span
        WHERE captured_at BETWEEN span.first_poll - INTERVAL '1 minute'
                              AND span.last_poll + INTERVAL '1 minute'
    )""",
    )
    bronze(
        """
        SELECT server, source_id, count(DISTINCT poll) AS polls, count(*) AS captured_rows
        FROM pass_capture GROUP BY server, source_id ORDER BY server
        """,
        PASS_CAPTURE,
    )
    return (PASS_CAPTURE,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Rows captured more than once in a poll

    The capture writes some legs several times in one poll. Each group below counts the legs
    captured that many times.
    """)
    return


@app.cell
def _(PASS_CAPTURE, bronze):
    bronze(
        """
        SELECT server, mode, copies, count(*) AS legs
        FROM (
            SELECT server, mode, count(*) AS copies
            FROM pass_capture
            GROUP BY server, mode, poll, trip_id, from_stop_id, scheduled_departure
        )
        GROUP BY server, mode, copies ORDER BY server, mode, copies
        """,
        PASS_CAPTURE,
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    `pass_leg` keeps one row per distinct leg in each poll, and strips each server's prefix from the
    trip and stop ids. `delfi_trip` is null for a trip from a feed other than DELFI.
    """)
    return


@app.cell
def _(PASS_CAPTURE, bronze):
    PASS_LEG = PASS_CAPTURE + (
        """
    pass_leg AS (
        SELECT DISTINCT
            server, poll,
            CASE WHEN trip_id ~ $delfi_trip THEN regexp_replace(trip_id, $delfi_trip, '') END
                AS delfi_trip,
            regexp_replace(from_stop_id, $delfi_stop, '') AS from_stop,
            regexp_replace(to_stop_id, $delfi_stop, '') AS to_stop,
            from_lat, from_lon, to_lat, to_lon,
            scheduled_departure, scheduled_arrival, departure, arrival, realtime,
            trip_id, route_name, mode, polyline
        FROM pass_capture
    )""",
    )
    bronze(
        """
        SELECT poll,
               count(*) FILTER (WHERE server = 'local') AS local_legs,
               count(*) FILTER (WHERE server = 'transitous') AS transitous_legs
        FROM pass_leg GROUP BY poll ORDER BY poll
        """,
        PASS_LEG,
    )
    return (PASS_LEG,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Every shape in the pass

    One path per distinct shape on each server: local in aqua, Transitous in magenta. Hover over a
    path for its leg. Both layers cover Thuringia and the lines leaving it.
    """)
    return


@app.cell
def _(PASS_LEG, bronze, server_map):
    pass_shapes = bronze(
        """
        SELECT DISTINCT server, route_name, mode, delfi_trip, from_stop, to_stop, polyline
        FROM pass_leg
        """,
        PASS_LEG,
    ).to_pandas()
    server_map(pass_shapes)
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Joined legs, and legs one server alone returned

    Only the polls both servers answered count. A leg joins across the whole pass rather than within
    one poll. Each poll asks for the legs running at its instant, by realtime, so a train late on one
    server and on time on the other appears in different polls. Each server's side of a joined leg is
    its sighting at the last poll in which both servers returned the leg. A leg the two servers never
    returned in the same poll takes each server's latest sighting instead. `in_same_poll` marks the
    joined legs both servers returned in at least one poll.

    A Transitous leg from a feed other than DELFI has no counterpart on the local server, so it is
    counted apart from the DELFI legs one server alone returned.
    """)
    return


@app.cell
def _(PASS_LEG, bronze):
    ANSWERED = PASS_LEG + (
        """
    answered AS (
        SELECT * FROM pass_leg
        WHERE poll IN (
            SELECT poll FROM pass_leg GROUP BY poll HAVING count(DISTINCT server) = 2
        )
    )""",
    )
    bronze(
        """
        SELECT poll, servers, coalesce(answered_legs, 0) AS answered_legs
        FROM (SELECT poll, count(DISTINCT server) AS servers FROM pass_leg GROUP BY poll)
        LEFT JOIN (SELECT poll, count(*) AS answered_legs FROM answered GROUP BY poll)
            USING (poll)
        ORDER BY poll
        """,
        ANSWERED,
    )
    return (ANSWERED,)


@app.cell
def _(mo):
    mo.md(r"""
    `shared_poll` lists, for each DELFI leg, the polls in which both servers returned it. `latest` keeps
    one sighting of each leg per server: the last shared poll, or the last poll for a leg with none.
    `legs` is at most `sightings`, and `at_shared_poll` counts the legs taken from a shared poll.
    """)
    return


@app.cell
def _(ANSWERED, bronze):
    SHARED_POLL = ANSWERED + (
        """
    shared_poll AS (
        SELECT delfi_trip, from_stop, scheduled_departure, poll
        FROM answered
        WHERE delfi_trip IS NOT NULL
        GROUP BY delfi_trip, from_stop, scheduled_departure, poll
        HAVING count(DISTINCT server) = 2
    )""",
    )
    bronze(
        """
        SELECT count(*) AS shared_polls,
               count(DISTINCT (delfi_trip, from_stop, scheduled_departure)) AS legs
        FROM shared_poll
        """,
        SHARED_POLL,
    )
    return (SHARED_POLL,)


@app.cell
def _(SHARED_POLL, bronze):
    LATEST = SHARED_POLL + (
        """
    latest AS (
        SELECT answered.*, shared_poll.poll IS NOT NULL AS at_shared_poll
        FROM answered LEFT JOIN shared_poll
            ON answered.delfi_trip = shared_poll.delfi_trip
           AND answered.from_stop = shared_poll.from_stop
           AND answered.scheduled_departure = shared_poll.scheduled_departure
           AND answered.poll = shared_poll.poll
        QUALIFY row_number() OVER (
            PARTITION BY server, trip_id, answered.from_stop, answered.scheduled_departure
            ORDER BY shared_poll.poll IS NOT NULL DESC, answered.poll DESC
        ) = 1
    )""",
    )
    bronze(
        """
        SELECT server, sightings, legs, at_shared_poll
        FROM (SELECT server, count(*) AS sightings FROM answered GROUP BY server)
        JOIN (
            SELECT server, count(*) AS legs,
                   sum(CASE WHEN at_shared_poll THEN 1 ELSE 0 END) AS at_shared_poll
            FROM latest GROUP BY server
        ) USING (server)
        ORDER BY server
        """,
        LATEST,
    )
    return (LATEST,)


@app.cell
def _(LATEST, bronze):
    SEEN_TOGETHER = LATEST + (
        """
    seen_together AS (
        SELECT DISTINCT delfi_trip, from_stop, scheduled_departure, true AS in_same_poll
        FROM shared_poll
    )""",
    )
    bronze(
        "SELECT count(*) AS legs_seen_together FROM seen_together",
        SEEN_TOGETHER,
    )
    return (SEEN_TOGETHER,)


@app.cell
def _(mo):
    mo.md(r"""
    Each side of the join, and Transitous's legs from other feeds. `repeated_keys` counts the join
    keys a side holds more than once. Any such key would join to several legs on the other side.
    """)
    return


@app.cell
def _(SEEN_TOGETHER, bronze):
    SIDES = SEEN_TOGETHER + (
        "local_leg AS (SELECT * FROM latest WHERE server = 'local')",
        """
    transitous_leg AS (
        SELECT * FROM latest WHERE server = 'transitous' AND delfi_trip IS NOT NULL
    )""",
        """
    other_feed AS (
        SELECT * FROM latest WHERE server = 'transitous' AND delfi_trip IS NULL
    )""",
    )
    bronze(
        " UNION ALL ".join(
            f"""
            SELECT '{_side}' AS side, count(*) AS legs,
                   (SELECT count(*) FROM (
                       SELECT 1 FROM {_side}
                       GROUP BY delfi_trip, from_stop, scheduled_departure
                       HAVING count(*) > 1
                   )) AS repeated_keys
            FROM {_side}
            """
            for _side in ("local_leg", "transitous_leg", "other_feed")
        ),
        SIDES,
    )
    return (SIDES,)


@app.cell
def _(SIDES, bronze):
    JOINED = SIDES + (
        """
    joined AS (
        SELECT
            coalesce(l.delfi_trip, t.delfi_trip) AS delfi_trip,
            coalesce(l.from_stop, t.from_stop) AS from_stop,
            coalesce(l.scheduled_departure, t.scheduled_departure) AS scheduled_departure,
            CASE
                WHEN t.trip_id IS NULL THEN 'local only'
                WHEN l.trip_id IS NULL THEN 'transitous only'
                ELSE 'joined'
            END AS outcome,
            coalesce(t.mode, l.mode) AS mode,
            coalesce(t.route_name, l.route_name) AS route_name,
            coalesce(t.scheduled_arrival, l.scheduled_arrival) AS scheduled_arrival,
            l.route_name AS local_route_name,
            l.poll AS local_poll, t.poll AS transitous_poll,
            l.to_stop AS local_to_stop, t.to_stop AS transitous_to_stop,
            l.arrival AS local_arrival, t.arrival AS transitous_arrival,
            l.realtime AS local_realtime, t.realtime AS transitous_realtime,
            l.polyline AS local_polyline, t.polyline AS transitous_polyline
        FROM local_leg AS l
        FULL JOIN transitous_leg AS t
            ON l.delfi_trip = t.delfi_trip
           AND l.from_stop = t.from_stop
           AND l.scheduled_departure = t.scheduled_departure
    )""",
    )
    bronze(
        "SELECT outcome, count(*) AS legs FROM joined GROUP BY outcome ORDER BY outcome",
        JOINED,
    )
    return (JOINED,)


@app.cell
def _(JOINED, bronze, pd):
    MATCHED_LEG = JOINED + (
        """
    matched_leg AS (
        SELECT joined.*, coalesce(seen_together.in_same_poll, false) AS in_same_poll
        FROM joined LEFT JOIN seen_together
            ON joined.delfi_trip = seen_together.delfi_trip
           AND joined.from_stop = seen_together.from_stop
           AND joined.scheduled_departure = seen_together.scheduled_departure
    )""",
    )
    matched = bronze("SELECT * FROM matched_leg", MATCHED_LEG).to_pandas()
    pd.crosstab(matched["outcome"], matched["in_same_poll"]).rename(
        columns=str
    )
    return MATCHED_LEG, matched


@app.cell
def _(MATCHED_LEG, bronze):
    outcome_counts = bronze(
        """
        SELECT mode,
               count(*) FILTER (WHERE outcome = 'joined') AS joined,
               count(*) FILTER (WHERE outcome = 'local only') AS local_only,
               count(*) FILTER (WHERE outcome = 'transitous only') AS transitous_only,
               count(*) FILTER (WHERE outcome = 'transitous, other feed') AS transitous_other_feed
        FROM (
            SELECT mode, outcome FROM matched_leg
            UNION ALL
            SELECT mode, 'transitous, other feed' FROM other_feed
        )
        GROUP BY mode ORDER BY mode
        """,
        MATCHED_LEG,
    )
    outcome_counts
    return


@app.cell
def _(matched):
    matched.loc[
        matched["outcome"] != "joined",
        [
            "outcome",
            "route_name",
            "mode",
            "delfi_trip",
            "from_stop",
            "scheduled_departure",
            "local_to_stop",
            "transitous_to_stop",
        ],
    ].sort_values(["scheduled_departure", "outcome"])
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### A route's legs on each server

    Every leg of the chosen route that either server returned, joined or not. The routes offered are
    those with a leg one server alone returned.
    """)
    return


@app.cell
def _(SERVER_COLOUR, matched, mo, pd):
    matched_shapes = pd.concat(
        [
            matched[matched[f"{server}_polyline"].notna()].assign(
                server=server,
                to_stop=matched[f"{server}_to_stop"],
                polyline=matched[f"{server}_polyline"],
            )[
                [
                    "server",
                    "outcome",
                    "route_name",
                    "mode",
                    "delfi_trip",
                    "from_stop",
                    "to_stop",
                    "scheduled_departure",
                    "polyline",
                ]
            ]
            for server in SERVER_COLOUR
        ]
    )
    unjoined_route = mo.ui.dropdown(
        sorted(
            matched.loc[matched["outcome"] != "joined", "route_name"].unique()
        ),
        label="route",
    )
    unjoined_route
    return matched_shapes, unjoined_route


@app.cell
def _(matched_shapes, mo, server_map, unjoined_route):
    mo.stop(unjoined_route.value is None, mo.md("Choose a route."))
    server_map(
        matched_shapes[matched_shapes["route_name"] == unjoined_route.value]
    )
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Each joined leg, compared

    `shape_distance` is the Hausdorff distance between the two servers' shapes, in metres: the
    furthest any point of one shape lies from the other. `arrival_difference` is Transitous's arrival
    less the local server's, in minutes, each from that server's latest sighting. Legs whose two sides
    end at different stops cover different track, so they are left out.
    """)
    return


@app.cell
def _(METRIC_CRS, matched, shape_of):
    _joined = matched[
        (matched["outcome"] == "joined")
        & (matched["local_to_stop"] == matched["transitous_to_stop"])
    ]
    compared = _joined.assign(
        shape_distance=shape_of(_joined["local_polyline"])
        .to_crs(METRIC_CRS)
        .hausdorff_distance(
            shape_of(_joined["transitous_polyline"]).to_crs(METRIC_CRS)
        ),
        arrival_difference=(
            _joined["transitous_arrival"] - _joined["local_arrival"]
        ).dt.total_seconds()
        / 60,
    )[
        [
            "mode",
            "route_name",
            "delfi_trip",
            "from_stop",
            "local_to_stop",
            "scheduled_departure",
            "local_realtime",
            "transitous_realtime",
            "shape_distance",
            "arrival_difference",
            "local_polyline",
            "transitous_polyline",
        ]
    ].reset_index(drop=True)
    compared.drop(columns=["local_polyline", "transitous_polyline"])
    return (compared,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Example shapes, by mode

    For each mode, the joined legs with the smallest, median, and largest shape distance. Local in
    aqua, Transitous in magenta, each panel at its own scale.
    """)
    return


@app.cell
def _(METRIC_CRS, SERVER_COLOUR, compared, pd, plt, shape_of):
    _examples = {
        _mode: _legs.sort_values("shape_distance").iloc[
            [0, len(_legs) // 2, len(_legs) - 1]
        ]
        for _mode, _legs in compared.groupby("mode")
    }
    _fig, _axes = plt.subplots(
        len(_examples), 3, figsize=(11, 3.4 * len(_examples)), squeeze=False
    )
    for _row, (_mode, _legs) in zip(_axes, _examples.items()):
        for _ax, (_, _leg), _rank in zip(
            _row, _legs.iterrows(), ("smallest", "median", "largest")
        ):
            for _server, _colour in SERVER_COLOUR.items():
                shape_of(pd.Series([_leg[f"{_server}_polyline"]])).to_crs(
                    METRIC_CRS
                ).plot(ax=_ax, color=_colour, linewidth=2)
            _ax.set_title(
                f"{_mode}, {_rank}\n{_leg['route_name']}: {_leg['shape_distance']:,.0f} m",
                fontsize=9,
                color="#0b0b0b",
            )
            _ax.set_aspect("equal")
            _ax.set_axis_off()
    _fig.tight_layout()
    _fig
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### Any joined leg on a map

    Select legs in the table to draw them. The leg with the largest shape distance in each mode
    starts selected.
    """)
    return


@app.cell
def _(compared, mo):
    compared_table = mo.ui.table(
        compared,
        hidden_columns=["local_polyline", "transitous_polyline"],
        initial_selection=[
            compared.index.get_loc(_index)
            for _index in compared.groupby("mode")["shape_distance"].idxmax()
        ],
        page_size=10,
    )
    compared_table
    return (compared_table,)


@app.cell
def _(SERVER_COLOUR, compared_table, mo, pd, server_map):
    mo.stop(compared_table.value.empty, mo.md("Select a leg."))
    server_map(
        pd.concat(
            [
                compared_table.value.assign(
                    server=_server,
                    polyline=compared_table.value[f"{_server}_polyline"],
                )
                for _server in SERVER_COLOUR
            ]
        )[
            [
                "server",
                "route_name",
                "mode",
                "delfi_trip",
                "from_stop",
                "local_to_stop",
                "scheduled_departure",
                "shape_distance",
                "polyline",
            ]
        ]
    )
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ### By mode

    One dot per joined leg. Colour marks whether the local server had realtime for the leg; Transitous
    had realtime for every leg shown in orange as well as most in blue.
    """)
    return


@app.cell
def _(compared, plt):
    _realtime_colour = {True: "#2a78d6", False: "#eb6834"}
    _modes = sorted(compared["mode"].unique())
    _fig, (_shape_ax, _arrival_ax) = plt.subplots(
        1, 2, figsize=(11, 3.2), sharey=True
    )
    for _row, _mode in enumerate(_modes):
        for _realtime, _colour in _realtime_colour.items():
            _legs = compared[
                (compared["mode"] == _mode)
                & (compared["local_realtime"] == _realtime)
            ]
            _offsets = (_legs.index.to_series().rank() % 7 - 3) * 0.06
            for _ax, _column in (
                (_shape_ax, "shape_distance"),
                (_arrival_ax, "arrival_difference"),
            ):
                _ax.scatter(
                    _legs[_column],
                    _row + _offsets,
                    s=40,
                    color=_colour,
                    edgecolors="white",
                    linewidths=1.5,
                    zorder=3,
                    label=(
                        "local realtime"
                        if _realtime
                        else "local scheduled only"
                    )
                    if _row == 0 or _ax is _shape_ax and _mode == _modes[-1]
                    else None,
                )
    _shape_ax.set_xscale("log")
    _shape_ax.set_xlabel("shape distance (m, log scale)")
    _arrival_ax.set_xlabel("arrival difference, Transitous − local (min)")
    _shape_ax.set_yticks(range(len(_modes)), _modes)
    for _ax in (_shape_ax, _arrival_ax):
        _ax.grid(axis="x", color="#e4e3df", linewidth=0.8, zorder=0)
        _ax.tick_params(colors="#52514e", length=0)
        _ax.xaxis.label.set_color("#52514e")
        for _side in ("top", "right", "left"):
            _ax.spines[_side].set_visible(False)
        _ax.spines["bottom"].set_color("#c9c8c2")
    _handles = [
        plt.Line2D(
            [],
            [],
            marker="o",
            linestyle="",
            markersize=7,
            color=_colour,
            markeredgecolor="white",
            label=_label,
        )
        for _colour, _label in (
            ("#2a78d6", "local realtime"),
            ("#eb6834", "local scheduled only"),
        )
    ]
    _fig.legend(
        handles=_handles,
        loc="upper center",
        ncols=2,
        frameon=False,
        labelcolor="#0b0b0b",
    )
    _fig.tight_layout(rect=(0, 0, 1, 0.9))
    _fig
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Towards agreement thresholds

    The quantiles of each measure by mode, and whether the local server had realtime. A threshold is
    drawn from the legs where the two servers start from the same information.
    """)
    return


@app.cell
def _(compared):
    agreement_quantiles = (
        compared.groupby(["mode", "local_realtime"])[
            ["shape_distance", "arrival_difference"]
        ]
        .describe(percentiles=[0.5, 0.9, 0.95])
        .drop(columns=["mean", "std"], level=1)
        .round(1)
    )
    agreement_quantiles
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Findings from the baseline pass

    The pass of 13 October 2026 ran from 21:51 to 23:51 UTC, at night, so it holds few trains: 160
    joined legs, 145 of them regional.

    **Joining.** Every regional leg joined. Of 30 long-distance and high-speed legs, 15 joined, 9
    came from the local server alone, and 4 from Transitous alone. Most of the unjoined legs belong
    to `12N`, which DELFI carries as several trips with identical times. The two servers return
    different ones, and split them at different stops. Transitous returns 4 more legs from feeds
    other than DELFI, all night trains.

    **Shapes.** Every local shape is a straight line between its stops. The distance to Transitous's
    track shape measures that straight line, not disagreement between two track shapes: a median
    of 519 m on regional legs, 8 km on high-speed legs, and 36 km on long-distance legs. This pass
    cannot set a shape threshold. The pass after `route_shapes` can.

    **Arrivals.** Where both servers had realtime, all 145 regional legs agree within 1 minute, 143
    of them exactly. The local server had no realtime for any high-speed leg, and Transitous had it
    for 5 of the 10, which differ by up to 71 minutes. One long-distance leg had realtime on both,
    yet Transitous forecast a delay of 7–11 minutes that the local server never showed.

    **Threshold.** Two arrivals agree when they differ by at most 1 minute.
    """)
    return


if __name__ == "__main__":
    app.run()
