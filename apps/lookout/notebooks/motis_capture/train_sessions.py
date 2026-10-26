# /// script
# dependencies = [
#     "altair==6.3.0",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo>=0.25.2",
#     "pandas==3.0.6",
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
    import altair as alt
    import geopandas as gpd
    import lonboard
    import pyarrow as pa

    import lookout_medallion

    return gpd, lonboard, lookout_medallion, mo, pa


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # One train as a session

    A train session replays a trip the Motis captures saw, as a device on board would have recorded it.
    It holds a sample every 10 s, placed along each leg's shape. Each sample's `geometry_source` says
    where its position came from: `Stop` at a leg's departure or arrival, where a capture states the
    train's place, and `Interpolated` everywhere else.

    This notebook picks one train and draws its samples over its legs, coloured by `geometry_source`.
    """)
    return


@app.cell
def _(lookout_medallion, mo, pa):
    COUNTRY = "DE"
    SHAPED_LABEL = "%route-shapes%"
    matches = pa.table(
        lookout_medallion.query_silver(
            "WITH recorded AS ("
            "  SELECT session_id, started_at, sample_count, bbox FROM session_v2 WHERE sample_count >= 50"
            "), boxes AS ("
            "  SELECT r.session_id AS recorded, t.session_id AS train, t.sample_count AS train_samples,"
            "    GREATEST(0, LEAST(r.bbox['xmax'], t.bbox['xmax']) - GREATEST(r.bbox['xmin'], t.bbox['xmin']))"
            "    * GREATEST(0, LEAST(r.bbox['ymax'], t.bbox['ymax']) - GREATEST(r.bbox['ymin'], t.bbox['ymin']))"
            "    AS shared,"
            "    (r.bbox['xmax'] - r.bbox['xmin']) * (r.bbox['ymax'] - r.bbox['ymin'])"
            "    + (t.bbox['xmax'] - t.bbox['xmin']) * (t.bbox['ymax'] - t.bbox['ymin']) AS both_areas"
            "  FROM recorded r JOIN train_session t ON t.source_label LIKE $shaped"
            "), candidates AS ("
            "  SELECT recorded, train, train_samples FROM ("
            "    SELECT *, ROW_NUMBER() OVER ("
            "      PARTITION BY recorded ORDER BY shared / (both_areas - shared) DESC"
            "    ) AS rank FROM boxes WHERE shared > 0"
            "  ) WHERE rank <= 20"
            "), pairs AS ("
            "  SELECT c.recorded, c.train, c.train_samples, s.seq AS recorded_seq, ts.seq AS train_seq"
            "  FROM candidates c JOIN session_sample_v2 s ON s.session_id = c.recorded"
            "  JOIN train_session_sample ts ON ts.session_id = c.train"
            "    AND ST_DWithin(s.geometry_projected, ts.geometry_projected, 300)"
            "), scored AS ("
            "  SELECT p.recorded, p.train,"
            "    CAST(COUNT(DISTINCT p.recorded_seq) AS DOUBLE) / r.sample_count AS recorded_share,"
            "    CAST(COUNT(DISTINCT p.train_seq) AS DOUBLE) / p.train_samples AS train_share"
            "  FROM pairs p JOIN recorded r ON r.session_id = p.recorded"
            "  GROUP BY p.recorded, p.train, p.train_samples, r.sample_count"
            "), best AS ("
            "  SELECT *, ROW_NUMBER() OVER ("
            "    PARTITION BY recorded ORDER BY recorded_share * train_share DESC"
            "  ) AS rank FROM scored"
            ") "
            "SELECT r.session_id AS recorded, r.started_at AS recorded_at, r.sample_count AS recorded_samples,"
            "  b.recorded_share, b.train_share, b.recorded_share * b.train_share AS score,"
            "  t.session_id, t.trip_id, t.source_label, t.sample_count "
            "FROM best b JOIN recorded r ON r.session_id = b.recorded "
            "JOIN train_session t ON t.session_id = b.train "
            "WHERE b.rank = 1 ORDER BY score DESC",
            country=COUNTRY,
            params={"shaped": SHAPED_LABEL},
        )
    )
    session_picker = mo.ui.table(
        matches.to_pandas(), selection="single", initial_selection=[0]
    )
    mo.vstack(
        [
            mo.md(
                f"{matches.num_rows} recorded sessions, each with the train from a source labelled "
                f"`{SHAPED_LABEL}` whose route best follows it, on any day. A sample is near when it "
                "lies within 300 m of one of the other's; the score multiplies the two shares near. "
                "Pick one to draw."
            ),
            session_picker,
        ]
    )
    return COUNTRY, session_picker


@app.cell
def _(COUNTRY, lookout_medallion, mo, pa, session_picker):
    mo.stop(session_picker.value.empty, mo.md("Pick a session above."))
    picked = session_picker.value.iloc[0]
    samples = pa.table(
        lookout_medallion.query_silver(
            "SELECT seq, t, geometry_source, implied_speed_mps, geometry, geometry_projected "
            "FROM train_session_sample WHERE session_id = $session ORDER BY seq",
            country=COUNTRY,
            params={"session": picked.session_id},
        )
    )
    _rows = samples.drop_columns(
        ["geometry", "geometry_projected"]
    ).to_pandas()
    mo.vstack(
        [
            mo.md(
                f"The train on trip `{picked.trip_id}`: {len(_rows)} samples, "
                f"{picked.sample_count} by its session row."
            ),
            _rows.groupby("geometry_source")
            .size()
            .rename("samples")
            .to_frame(),
            mo.hstack([_rows.head(5), _rows.tail(5)]),
        ]
    )
    return picked, samples


@app.cell
def _(COUNTRY, lookout_medallion, mo, pa, picked):
    recorded = pa.table(
        lookout_medallion.query_silver(
            "SELECT seq, t, acc, geometry FROM session_sample_v2 "
            "WHERE session_id = $session ORDER BY seq",
            country=COUNTRY,
            params={"session": picked.recorded},
        )
    )
    mo.md(
        f"The recording `{picked.recorded}`: {recorded.num_rows} samples, from "
        f"{recorded['t'][0].as_py():%d %b %H:%M} to {recorded['t'][-1].as_py():%H:%M} UTC. "
        f"{picked.recorded_share:.0%} of them lie within 300 m of the train's samples, and "
        f"{picked.train_share:.0%} of the train's within 300 m of theirs."
    )
    return (recorded,)


@app.cell
def _(mo):
    shown = mo.ui.multiselect(
        options=["interpolated", "stop", "recorded"],
        value=["interpolated", "stop", "recorded"],
        label="Show",
    )
    shown
    return (shown,)


@app.cell
def _(
    COUNTRY,
    gpd,
    lonboard,
    lookout_medallion,
    mo,
    pa,
    picked,
    recorded,
    samples,
):
    SAMPLE_COLOURS = {"interpolated": [42, 120, 214], "stop": [235, 104, 52]}
    RECORDED_COLOUR = [27, 175, 122]

    legs = pa.table(
        lookout_medallion.query_silver(
            "SELECT from_stop_id, departure, arrival, geometry FROM train_segment_v3 "
            "WHERE trip_id = $trip AND source_id = "
            "(SELECT source_id FROM train_session WHERE session_id = $session) "
            "ORDER BY departure",
            country=COUNTRY,
            params={"trip": picked.trip_id, "session": picked.session_id},
        )
    )
    _legs = gpd.GeoDataFrame.from_arrow(legs)
    _points = gpd.GeoDataFrame.from_arrow(
        samples.drop_columns("geometry_projected")
    )
    _recorded = gpd.GeoDataFrame.from_arrow(recorded)
    point_layers = {
        "recorded": lonboard.ScatterplotLayer.from_geopandas(
            _recorded,
            get_fill_color=RECORDED_COLOUR,
            radius_units="pixels",
            get_radius=6,
        ),
        **{
            source: lonboard.ScatterplotLayer.from_geopandas(
                _points[_points["geometry_source"] == source],
                get_fill_color=colour,
                radius_units="pixels",
                get_radius=radius,
            )
            for (source, colour), radius in zip(SAMPLE_COLOURS.items(), [4, 7])
        },
    }

    mo.vstack(
        [
            mo.md(
                f"{len(_points)} train samples over {len(_legs)} legs, and {len(_recorded)} recorded. "
                + " · ".join(
                    f'<span style="color: rgb{tuple(colour)}">●</span> {source}'
                    for source, colour in [
                        *SAMPLE_COLOURS.items(),
                        ("recorded", RECORDED_COLOUR),
                    ]
                )
            ),
            lonboard.Map(
                [
                    lonboard.PathLayer.from_geopandas(
                        _legs,
                        get_color=[120, 120, 120],
                        width_units="pixels",
                        get_width=2,
                    ),
                    *point_layers.values(),
                ],
                basemap=lonboard.basemap.MaplibreBasemap(
                    style=lonboard.basemap.CartoStyle.Positron
                ),
            ),
        ]
    )
    return (point_layers,)


@app.cell
def _(point_layers, shown):
    for _source, _layer in point_layers.items():
        _layer.visible = _source in shown.value
    return


if __name__ == "__main__":
    app.run()
