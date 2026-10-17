# /// script
# dependencies = [
#     "geopandas==1.1.4",
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
    import matplotlib.pyplot as plt
    import pandas as pd
    import polyline
    import pyarrow as pa
    from shapely.geometry import LineString

    import lookout_medallion

    return LineString, gpd, lookout_medallion, mo, pa, pd, plt, polyline


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
def _(lookout_medallion, pa, pd):
    MEDALLION_ROOT = lookout_medallion.default_root()
    LOCAL_LABEL = "delfi-2026-10-10.plain"
    DELFI_TRIP = r"^.*_(de-DELFI|germanygtfs)_"
    DELFI_STOP = r"^(de-DELFI|germanygtfs)_"
    POLYLINE_PRECISION = 5
    METRIC_CRS = "EPSG:25832"

    def bronze(sql: str, **params) -> pd.DataFrame:
        return pa.table(
            lookout_medallion.query_bronze(
                sql, params=params, root=str(MEDALLION_ROOT)
            )
        ).to_pandas()

    return (
        DELFI_STOP,
        DELFI_TRIP,
        LOCAL_LABEL,
        METRIC_CRS,
        POLYLINE_PRECISION,
        bronze,
    )


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## The pass

    The local source is the one carrying the pass's label. The Transitous source is the one that
    polled the same region over the same span. Both pollers started together, so a poll from each
    lands within a second of the other, and rounding each instant to the minute pairs them.
    """)
    return


@app.cell
def _(DELFI_STOP, DELFI_TRIP, LOCAL_LABEL, bronze):
    PASS_PARAMS = dict(
        label=LOCAL_LABEL, delfi_trip=DELFI_TRIP, delfi_stop=DELFI_STOP
    )
    PASS_LEG = """
    local_source AS (
        SELECT source_id, region_id FROM motis_source WHERE feed = 'local' AND label = $label
    ),
    span AS (
        SELECT min(captured_at) AS first_poll, max(captured_at) AS last_poll
        FROM motis_segment_v2 WHERE source_id IN (SELECT source_id FROM local_source)
    ),
    pass_source AS (
        SELECT source_id, 'local' AS server FROM local_source
        UNION ALL
        SELECT source_id, 'transitous' AS server FROM motis_source
        WHERE feed = 'transitous' AND region_id IN (SELECT region_id FROM local_source)
    ),
    pass_leg AS (
        SELECT DISTINCT
            server,
            date_trunc('minute', captured_at + INTERVAL '30 seconds') AS poll,
            CASE WHEN trip_id ~ $delfi_trip THEN regexp_replace(trip_id, $delfi_trip, '') END
                AS delfi_trip,
            regexp_replace(from_stop_id, $delfi_stop, '') AS from_stop,
            regexp_replace(to_stop_id, $delfi_stop, '') AS to_stop,
            scheduled_departure, scheduled_arrival, departure, arrival, realtime,
            trip_id, route_name, mode, polyline
        FROM motis_segment_v2 JOIN pass_source USING (source_id), span
        WHERE captured_at BETWEEN span.first_poll - INTERVAL '1 minute'
                              AND span.last_poll + INTERVAL '1 minute'
    )
    """
    bronze(
        f"""
        WITH {PASS_LEG}
        SELECT poll,
               count(*) FILTER (WHERE server = 'local') AS local_legs,
               count(*) FILTER (WHERE server = 'transitous') AS transitous_legs
        FROM pass_leg GROUP BY poll ORDER BY poll
        """,
        **PASS_PARAMS,
    )
    return PASS_LEG, PASS_PARAMS


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Joined legs, and legs one server alone returned

    Only the polls both servers answered count. A leg joins across the whole pass rather than within one poll. Each poll asks for the legs running
    at its instant, by realtime, so a train late on one server and on time on the other appears in
    different polls. Each server's side of a joined leg is its latest sighting of that leg.
    `in_same_poll` marks the joined legs both servers returned in at least one poll.

    A Transitous leg from a feed other than DELFI has no counterpart on the local server, so it is
    counted apart from the DELFI legs one server alone returned.
    """)
    return


@app.cell
def _(PASS_LEG, PASS_PARAMS, bronze):
    MATCHED_LEG = """
    answered AS (
        SELECT * FROM pass_leg
        WHERE poll IN (
            SELECT poll FROM pass_leg GROUP BY poll HAVING count(DISTINCT server) = 2
        )
    ),
    latest AS (
        SELECT * FROM answered
        QUALIFY row_number() OVER (
            PARTITION BY server, trip_id, from_stop, scheduled_departure ORDER BY poll DESC
        ) = 1
    ),
    seen_together AS (
        SELECT DISTINCT delfi_trip, from_stop, scheduled_departure, true AS in_same_poll
        FROM answered
        WHERE delfi_trip IS NOT NULL
        GROUP BY delfi_trip, from_stop, scheduled_departure, poll
        HAVING count(DISTINCT server) = 2
    ),
    l AS (SELECT * FROM latest WHERE server = 'local'),
    t AS (SELECT * FROM latest WHERE server = 'transitous' AND delfi_trip IS NOT NULL),
    other_feed AS (SELECT * FROM latest WHERE server = 'transitous' AND delfi_trip IS NULL),
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
        FROM l
        FULL JOIN t
            ON l.delfi_trip = t.delfi_trip
           AND l.from_stop = t.from_stop
           AND l.scheduled_departure = t.scheduled_departure
    ),
    matched_leg AS (
        SELECT joined.*, coalesce(seen_together.in_same_poll, false) AS in_same_poll
        FROM joined LEFT JOIN seen_together
            ON joined.delfi_trip = seen_together.delfi_trip
           AND joined.from_stop = seen_together.from_stop
           AND joined.scheduled_departure = seen_together.scheduled_departure
    )
    """
    matched = bronze(
        f"WITH {PASS_LEG}, {MATCHED_LEG} SELECT * FROM matched_leg",
        **PASS_PARAMS,
    )
    outcome_counts = bronze(
        f"""
        WITH {PASS_LEG}, {MATCHED_LEG}
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
        **PASS_PARAMS,
    )
    outcome_counts
    return (matched,)


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
def _(LineString, METRIC_CRS, POLYLINE_PRECISION, gpd, matched, pd, polyline):
    def _shape(encoded: pd.Series) -> gpd.GeoSeries:
        return gpd.GeoSeries(
            [
                LineString(
                    polyline.decode(line, POLYLINE_PRECISION, geojson=True)
                )
                for line in encoded
            ],
            index=encoded.index,
            crs="EPSG:4326",
        ).to_crs(METRIC_CRS)

    _joined = matched[
        (matched["outcome"] == "joined")
        & (matched["local_to_stop"] == matched["transitous_to_stop"])
    ]
    compared = _joined.assign(
        shape_distance=_shape(_joined["local_polyline"]).hausdorff_distance(
            _shape(_joined["transitous_polyline"])
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
        ]
    ]
    compared
    return (compared,)


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
    for 7 of the 10, which differ by up to 71 minutes. One long-distance leg had realtime on both,
    yet Transitous forecast a delay of 7–11 minutes that the local server never showed.

    **Threshold.** Two arrivals agree when they differ by at most 1 minute.
    """)
    return


if __name__ == "__main__":
    app.run()
