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


@app.cell
def _(mo):
    mo.md(r"""
    # Two passes compared: `delfi-2026-10-10.plain` and `delfi-2026-10-10.route-shapes`

    The local Motis server against Transitous, over two passes: before and after a change to the local
    import. Both passes polled every train in Thuringia every 10 minutes, from the DELFI build of 10
    October 2026. For the second pass, the local import computed rail shapes along OSM track with
    `route_shapes`, and named trains with Transitous's `de-DELFI.lua`.

    Each pass runs through the same steps as its side-by-side notebook. Every preview shows both
    passes, each row marked with its pass.

    Train data from [Transitous](https://transitous.org/) and the
    [sources it draws on](https://transitous.org/sources/).
    """)
    return


@app.cell
def _(LineString, gpd, lonboard, lookout_medallion, pa, pd, polyline):
    MEDALLION_ROOT = lookout_medallion.default_root()
    PASSES = {
        "baseline": "delfi-2026-10-10.plain",
        "route-shapes": "delfi-2026-10-10.route-shapes",
    }
    DELFI_TRIP = r"^.*_(de-DELFI|germanygtfs)_"
    DELFI_STOP = r"^(de-DELFI|germanygtfs)_"
    POLYLINE_PRECISION = 5
    # Motis encodes each shape as a Google polyline, whose points are WGS 84 latitude and longitude.
    POLYLINE_CRS = "EPSG:4326"
    # Shape distances need a CRS in metres. The store projects German data to ETRS89 / UTM zone 32N.
    METRIC_CRS = lookout_medallion.projected_crs("DE")
    SERVER_COLOUR = {"local": "#1baf7a", "transitous": "#e87ba4"}
    PASS_COLOUR = {"baseline": "#2a78d6", "route-shapes": "#eb6834"}

    def bronze(sql: str, steps: tuple[str, ...], label: str) -> pa.Table:
        if steps:
            sql = "WITH " + ",\n".join(steps) + "\n" + sql
        return pa.table(
            lookout_medallion.query_bronze(
                sql,
                params=dict(
                    label=label,
                    delfi_trip=DELFI_TRIP,
                    delfi_stop=DELFI_STOP,
                ),
                root=str(MEDALLION_ROOT),
            )
        )

    def per_pass(sql: str, steps: tuple[str, ...] = ()) -> pa.Table:
        return pa.concat_tables(
            [
                (_table := bronze(sql, steps, label)).add_column(
                    0, "pass", pa.array([name] * _table.num_rows, pa.string())
                )
                for name, label in PASSES.items()
            ],
            promote_options="permissive",
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

    def shape_map(
        legs: pd.DataFrame,
        layer: str,
        colours: dict[str, str],
        encoded: str = "polyline",
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
                    _legs[_legs[layer] == value],
                    get_color=[
                        int(colour[index : index + 2], 16)
                        for index in (1, 3, 5)
                    ],
                    width_units="pixels",
                    get_width=3,
                )
                for value, colour in colours.items()
                if (_legs[layer] == value).any()
            ]
        )

    return (
        METRIC_CRS,
        PASS_COLOUR,
        SERVER_COLOUR,
        per_pass,
        shape_map,
        shape_of,
    )


@app.cell
def _(mo):
    mo.md(r"""
    ## Each pass

    A pass's local source is the one carrying its label. Its Transitous source is the one that polled
    the same region over the same span. Both pollers started together, so a poll from each lands
    within a second of the other, and rounding each instant to the minute pairs them.

    Each step below adds one named query to those before it, runs it once per pass, and shows what it
    returns.
    """)
    return


@app.cell
def _(per_pass):
    LOCAL_SOURCE = (
        """
    local_source AS (
        SELECT source_id, region_id FROM motis_source WHERE feed = 'local' AND label = $label
    )""",
    )
    per_pass("SELECT * FROM local_source", LOCAL_SOURCE)
    return (LOCAL_SOURCE,)


@app.cell
def _(LOCAL_SOURCE, per_pass):
    SPAN = LOCAL_SOURCE + (
        """
    span AS (
        SELECT min(captured_at) AS first_poll, max(captured_at) AS last_poll
        FROM motis_segment_v2 WHERE source_id IN (SELECT source_id FROM local_source)
    )""",
    )
    per_pass("SELECT * FROM span", SPAN)
    return (SPAN,)


@app.cell
def _(mo):
    mo.md(r"""
    Every Transitous source for the region is a candidate in each pass. Only the one that polled
    within that pass's span contributes legs, at the next step.
    """)
    return


@app.cell
def _(SPAN, per_pass):
    PASS_SOURCE = SPAN + (
        """
    pass_source AS (
        SELECT source_id, 'local' AS server FROM local_source
        UNION ALL
        SELECT source_id, 'transitous' AS server FROM motis_source
        WHERE feed = 'transitous' AND region_id IN (SELECT region_id FROM local_source)
    )""",
    )
    per_pass(
        """
        SELECT server, source_id, base_url, overture_release, registered_at
        FROM pass_source JOIN motis_source USING (source_id)
        ORDER BY server, registered_at
        """,
        PASS_SOURCE,
    )
    return (PASS_SOURCE,)


@app.cell
def _(mo):
    mo.md(r"""
    `pass_capture` holds every row captured within a minute of the span, with its server and its
    poll. The span keeps each pass to its own captures, though both passes share a Transitous source.
    """)
    return


@app.cell
def _(PASS_SOURCE, per_pass):
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
    per_pass(
        """
        SELECT server, source_id, count(DISTINCT poll) AS polls, count(*) AS captured_rows,
               min(poll) AS first_poll, max(poll) AS last_poll
        FROM pass_capture GROUP BY server, source_id ORDER BY server
        """,
        PASS_CAPTURE,
    )
    return (PASS_CAPTURE,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Rows captured more than once in a poll

    Each group below counts the legs captured that many times in one poll. The next step keeps one
    copy of each.
    """)
    return


@app.cell
def _(PASS_CAPTURE, per_pass):
    per_pass(
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
def _(PASS_CAPTURE, per_pass):
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
    per_pass(
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
    ### Every shape in both passes

    One path per distinct shape: the baseline's local server in blue, the route-shapes pass's local
    server in orange, and Transitous in magenta, from both passes. Hover over a path for its leg. The
    blue paths run straight between stops, and the orange ones follow the track.
    """)
    return


@app.cell
def _(PASS_COLOUR, PASS_LEG, SERVER_COLOUR, per_pass, shape_map):
    SHAPE_COLOUR = {
        "baseline, local": PASS_COLOUR["baseline"],
        "route-shapes, local": PASS_COLOUR["route-shapes"],
        "transitous": SERVER_COLOUR["transitous"],
    }
    _shapes = per_pass(
        """
        SELECT DISTINCT server, route_name, mode, delfi_trip, from_stop, to_stop, polyline
        FROM pass_leg
        """,
        PASS_LEG,
    ).to_pandas()
    pass_shapes = _shapes.assign(
        shape_source=(_shapes["pass"] + ", " + _shapes["server"]).where(
            _shapes["server"] == "local", "transitous"
        )
    )
    shape_map(pass_shapes, "shape_source", SHAPE_COLOUR)
    return (SHAPE_COLOUR,)


@app.cell
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
def _(PASS_LEG, per_pass):
    ANSWERED = PASS_LEG + (
        """
    answered AS (
        SELECT * FROM pass_leg
        WHERE poll IN (
            SELECT poll FROM pass_leg GROUP BY poll HAVING count(DISTINCT server) = 2
        )
    )""",
    )
    per_pass(
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
def _(ANSWERED, per_pass):
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
    per_pass(
        """
        SELECT count(*) AS shared_polls,
               count(DISTINCT (delfi_trip, from_stop, scheduled_departure)) AS legs
        FROM shared_poll
        """,
        SHARED_POLL,
    )
    return (SHARED_POLL,)


@app.cell
def _(SHARED_POLL, per_pass):
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
    per_pass(
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
def _(LATEST, per_pass):
    SEEN_TOGETHER = LATEST + (
        """
    seen_together AS (
        SELECT DISTINCT delfi_trip, from_stop, scheduled_departure, true AS in_same_poll
        FROM shared_poll
    )""",
    )
    per_pass(
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
def _(SEEN_TOGETHER, per_pass):
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
    per_pass(
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
def _(SIDES, per_pass):
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
    per_pass(
        "SELECT outcome, count(*) AS legs FROM joined GROUP BY outcome ORDER BY outcome",
        JOINED,
    )
    return (JOINED,)


@app.cell
def _(JOINED, pd, per_pass):
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
    matched = per_pass("SELECT * FROM matched_leg", MATCHED_LEG).to_pandas()
    pd.crosstab(
        [matched["pass"], matched["outcome"]], matched["in_same_poll"]
    ).rename(columns=str)
    return MATCHED_LEG, matched


@app.cell
def _(MATCHED_LEG, per_pass):
    outcome_counts = per_pass(
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
            "pass",
            "outcome",
            "route_name",
            "mode",
            "delfi_trip",
            "from_stop",
            "scheduled_departure",
            "local_to_stop",
            "transitous_to_stop",
        ],
    ].sort_values(["pass", "scheduled_departure", "outcome"])
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### A route's legs on each server

    Every leg of the chosen route that either server returned in the chosen pass, joined or not. Local
    in aqua, Transitous in magenta. The routes offered are those with a leg one server alone returned.
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
                    "pass",
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
    _routes = {
        f"{_pass}: {_route}": (_pass, _route)
        for _pass, _route in matched.loc[
            matched["outcome"] != "joined", ["pass", "route_name"]
        ]
        .drop_duplicates()
        .sort_values(["pass", "route_name"])
        .itertuples(index=False)
    }
    unjoined_route = mo.ui.dropdown(
        _routes, value=next(iter(_routes)), label="pass and route"
    )
    unjoined_route
    return matched_shapes, unjoined_route


@app.cell
def _(SERVER_COLOUR, matched_shapes, mo, shape_map, unjoined_route):
    mo.stop(unjoined_route.value is None, mo.md("Choose a route."))
    shape_map(
        matched_shapes[
            (matched_shapes["pass"] == unjoined_route.value[0])
            & (matched_shapes["route_name"] == unjoined_route.value[1])
        ],
        "server",
        SERVER_COLOUR,
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Each joined leg, compared

    `shape_distance` is the Hausdorff distance between the two servers' shapes, in metres: the
    furthest any point of one shape lies from the other. `arrival_difference` is Transitous's arrival
    less the local server's, in minutes, each from the sighting the join chose. Legs whose two sides
    end at different stops cover different track, so they are left out.
    """)
    return


@app.cell
def _(METRIC_CRS, matched, pd, shape_of):
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
            "pass",
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
    pd.crosstab(compared["mode"], compared["pass"])
    return (compared,)


@app.cell
def _(mo):
    mo.md(r"""
    ## The two passes, by mode

    One dot per joined leg, coloured by pass: the baseline in blue, the route-shapes pass in orange.
    Shape distance runs on a symmetric log axis, linear below 1 m, so a leg whose shapes coincide
    still shows, at 0.
    """)
    return


@app.cell
def _(PASS_COLOUR, compared, plt):
    _modes = sorted(compared["mode"].unique())
    _fig, (_shape_ax, _arrival_ax) = plt.subplots(
        1, 2, figsize=(11, 3.6), sharey=True
    )
    for _row, _mode in enumerate(_modes):
        for _lane, (_pass, _colour) in zip((-0.18, 0.18), PASS_COLOUR.items()):
            _legs = compared[
                (compared["mode"] == _mode) & (compared["pass"] == _pass)
            ]
            _offsets = (
                _row + _lane + (_legs.index.to_series().rank() % 5 - 2) * 0.03
            )
            for _ax, _column in (
                (_shape_ax, "shape_distance"),
                (_arrival_ax, "arrival_difference"),
            ):
                _ax.scatter(
                    _legs[_column],
                    _offsets,
                    s=36,
                    color=_colour,
                    edgecolors="white",
                    linewidths=1.2,
                    zorder=3,
                )
    _shape_ax.set_xscale("symlog", linthresh=1)
    _shape_ax.set_xlim(left=-0.2)
    _shape_ax.set_xlabel("shape distance (m, symmetric log scale)")
    _arrival_ax.set_xlabel("arrival difference, Transitous − local (min)")
    _shape_ax.set_yticks(range(len(_modes)), _modes)
    for _ax in (_shape_ax, _arrival_ax):
        _ax.grid(axis="x", color="#e4e3df", linewidth=0.8, zorder=0)
        _ax.tick_params(colors="#52514e", length=0)
        _ax.xaxis.label.set_color("#52514e")
        for _side in ("top", "right", "left"):
            _ax.spines[_side].set_visible(False)
        _ax.spines["bottom"].set_color("#c9c8c2")
    _fig.legend(
        handles=[
            plt.Line2D(
                [],
                [],
                marker="o",
                linestyle="",
                markersize=7,
                color=_colour,
                markeredgecolor="white",
                label=_pass,
            )
            for _pass, _colour in PASS_COLOUR.items()
        ],
        loc="upper center",
        ncols=2,
        frameon=False,
        labelcolor="#0b0b0b",
    )
    _fig.tight_layout(rect=(0, 0, 1, 0.9))
    _fig
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### Quantiles, pass by pass

    The quantiles of each measure by mode and by whether the local server had realtime, with the two
    passes on adjacent rows.
    """)
    return


@app.cell
def _(compared):
    agreement_quantiles = (
        compared.groupby(["mode", "local_realtime", "pass"])[
            ["shape_distance", "arrival_difference"]
        ]
        .describe(percentiles=[0.5, 0.9, 0.95])
        .drop(columns=["mean", "std"], level=1)
        .round(1)
    )
    agreement_quantiles
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## The same track in both passes

    A segment is a mode, a departure stop, and an arrival stop. The segments below were compared in
    both passes, which ran on different days, so each pass contributes its earliest leg on the
    segment. Train names changed between the passes with `de-DELFI.lua`, so the segment ignores the
    route name, and the table shows each pass's.
    """)
    return


@app.cell
def _(compared):
    _segment = ["mode", "from_stop", "local_to_stop"]
    _earliest = compared.sort_values("scheduled_departure").drop_duplicates(
        ["pass", *_segment]
    )
    _wide = _earliest.pivot(
        index=_segment,
        columns="pass",
        values=[
            "route_name",
            "shape_distance",
            "local_polyline",
            "transitous_polyline",
        ],
    ).dropna()
    _wide.columns = [
        f"{_pass.replace('-', '_')}_{_column}"
        for _column, _pass in _wide.columns
    ]
    shared_segments = _wide.reset_index().astype(
        {
            "baseline_shape_distance": float,
            "route_shapes_shape_distance": float,
        }
    )
    shared_segments.groupby("mode")[
        ["baseline_shape_distance", "route_shapes_shape_distance"]
    ].describe(percentiles=[0.5]).drop(columns=["mean", "std"], level=1).round(
        1
    )
    return (shared_segments,)


@app.cell
def _(mo):
    mo.md(r"""
    ### Example segments, by mode

    For each mode, the shared segments with the smallest, median, and largest baseline shape distance.
    In each panel, Transitous's shape from the route-shapes pass is the wide magenta line. The
    baseline's local shape is blue, and the route-shapes pass's local shape is orange, drawn over it.
    """)
    return


@app.cell
def _(METRIC_CRS, SHAPE_COLOUR, pd, plt, shape_of, shared_segments):
    _layers = {
        "route_shapes_transitous_polyline": (SHAPE_COLOUR["transitous"], 5),
        "baseline_local_polyline": (SHAPE_COLOUR["baseline, local"], 1.5),
        "route_shapes_local_polyline": (
            SHAPE_COLOUR["route-shapes, local"],
            1.5,
        ),
    }
    _examples = {
        _mode: _segments.sort_values("baseline_shape_distance").iloc[
            [0, len(_segments) // 2, len(_segments) - 1]
        ]
        for _mode, _segments in shared_segments.groupby("mode")
    }
    _fig, _axes = plt.subplots(
        len(_examples), 3, figsize=(11, 3.6 * len(_examples)), squeeze=False
    )
    for _row, (_mode, _segments) in zip(_axes, _examples.items()):
        for _ax, (_, _segment), _rank in zip(
            _row, _segments.iterrows(), ("smallest", "median", "largest")
        ):
            for _column, (_colour, _width) in _layers.items():
                shape_of(pd.Series([_segment[_column]])).to_crs(
                    METRIC_CRS
                ).plot(ax=_ax, color=_colour, linewidth=_width)
            _ax.set_title(
                f"{_mode}, {_rank}\n{_segment['route_shapes_route_name']}: "
                f"{_segment['baseline_shape_distance']:,.0f} m → "
                f"{_segment['route_shapes_shape_distance']:,.0f} m",
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
    ### Any shared segment on a map

    Select segments in the table to draw them, coloured as above. The segment with the largest
    baseline shape distance in each mode starts selected.
    """)
    return


@app.cell
def _(mo, shared_segments):
    segment_table = mo.ui.table(
        shared_segments,
        hidden_columns=list(shared_segments.filter(like="polyline").columns),
        initial_selection=[
            shared_segments.index.get_loc(_index)
            for _index in shared_segments.groupby("mode")[
                "baseline_shape_distance"
            ].idxmax()
        ],
        page_size=10,
    )
    segment_table
    return (segment_table,)


@app.cell
def _(SHAPE_COLOUR, mo, pd, segment_table, shape_map):
    mo.stop(segment_table.value.empty, mo.md("Select a segment."))
    shape_map(
        pd.concat(
            [
                segment_table.value.assign(
                    shape_source=_source,
                    polyline=segment_table.value[_column],
                )
                for _source, _column in (
                    ("baseline, local", "baseline_local_polyline"),
                    ("route-shapes, local", "route_shapes_local_polyline"),
                    ("transitous", "route_shapes_transitous_polyline"),
                )
            ]
        )[
            [
                "shape_source",
                "mode",
                "from_stop",
                "local_to_stop",
                "baseline_route_name",
                "route_shapes_route_name",
                "polyline",
            ]
        ],
        "shape_source",
        SHAPE_COLOUR,
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    ### Realtime on each server

    For each mode and pass, how many of the compared legs each server returned with realtime.
    """)
    return


@app.cell
def _(compared):
    compared.groupby(["mode", "pass"]).agg(
        legs=("pass", "size"),
        local_realtime=("local_realtime", "sum"),
        transitous_realtime=("transitous_realtime", "sum"),
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Findings

    The baseline ran at night, from 21:51 to 23:51 UTC on 13 October. The route-shapes pass ran at
    midday, from 10:53 to 12:43 UTC on 17 October, and holds 1,139 joined legs against the baseline's
    160.

    **Shapes.** In the route-shapes pass, every joined leg lies within 12.6 m of Transitous's shape.
    The baseline's medians were 519 m on regional legs, 8 km on high-speed legs, and 36 km on
    long-distance legs. On the 119 segments both passes compared, the regional median falls from
    520 m to 0 m, and the high-speed median from 8 km to 1.3 m. The two servers' shapes are identical
    on 84% of regional legs and 65% of high-speed legs.

    **Joining.** Every regional leg joined in both passes. The route-shapes pass leaves 10 high-speed
    and long-distance legs unjoined, against 13 in the baseline. Five of its six local-only legs depart
    from 12:22 UTC on, in the pass's last half hour. All four Transitous-only legs departed between
    09:49 and 10:31 UTC, before the pass began. One of them, FLX10, carries stop ids with a `_G`
    suffix the local server does not use.

    **Arrivals.** Regional arrivals agree in both passes, with a 95th percentile of 0 minutes in each.
    The widest regional difference is −6 to +4 minutes, in the route-shapes pass. Transitous had
    realtime for all 40 high-speed legs in the route-shapes pass, and the local server for 9. Without
    local realtime, high-speed arrivals differ by at most 13 minutes, against 71 in the baseline. With
    it, they differ by a median of 7 minutes. The three largest differences, 19 to 38 minutes, are all
    on ICE 599, whose delay Transitous carried and the local DELFI GTFS-RT feed did not.

    **For a shape threshold.** The largest shape distance in the route-shapes pass is 12.6 m, on both
    regional and high-speed legs.
    """)
    return


if __name__ == "__main__":
    app.run()
