# /// script
# dependencies = [
#     "altair==6.3.0",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "lookout-session-crossings",
#     "marimo>=0.25.1",
#     "pandas==3.0.6",
#     "pyarrow==25.0.0",
#     "shapely==2.1.2",
# ]
# requires-python = ">=3.13"
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# lookout-session-crossings = { path = "../../crates/session-crossings-py" }
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
    import lookout_session_crossings

    return (
        alt,
        gpd,
        lonboard,
        lookout_medallion,
        lookout_session_crossings,
        mo,
        pa,
    )


@app.cell
def _(mo):
    mo.md(r"""
    # Passes over a crossing

    How the session crossings matching finds each pass, shown step by step on the cases its geo test
    checks. Every step comes from the matching's own code, through the `lookout_session_crossings`
    bridge: the samples near a crossing, the visits they form, and one pass per visit.
    """)
    return


@app.cell
def _(gpd, lonboard, mo):
    CASES_PATH = (
        mo.notebook_dir()
        / "../../crates/session_crossings/tests/passes.geojson"
    )
    cases = gpd.read_file(CASES_PATH)
    mo.vstack(
        [
            cases.drop(columns="geometry"),
            lonboard.Map(
                lonboard.ScatterplotLayer.from_geopandas(
                    cases,
                    get_fill_color=[42, 120, 214],
                    radius_units="pixels",
                    get_radius=8,
                )
            ),
        ]
    )
    return (cases,)


@app.cell
def _(cases, mo):
    case_picker = mo.ui.dropdown(
        options=list(cases["name"]), value=cases["name"].iloc[0], label="Case"
    )
    case_picker
    return (case_picker,)


@app.cell
def _(case_picker, cases, lookout_medallion, pa):
    case = cases.set_index("name").loc[case_picker.value]
    samples = pa.table(
        lookout_medallion.query_silver(
            "SELECT t, acc, geometry, geometry_projected FROM session_sample "
            "WHERE session_id = $session ORDER BY t",
            country=case.country,
            params={"session": case.session_id},
        )
    )
    crossing = pa.table(
        lookout_medallion.query_silver(
            "SELECT crossing_id, water_class, rail_class, geometry, geometry_projected "
            "FROM water_crossing WHERE crossing_id = $crossing",
            country=case.country,
            params={"crossing": case.crossing_id},
        )
    )
    return case, crossing, samples


@app.cell
def _(crossing, gpd, lonboard, mo, samples):
    _samples = gpd.GeoDataFrame.from_arrow(
        samples.drop_columns("geometry_projected")
    )
    _crossing = gpd.GeoDataFrame.from_arrow(
        crossing.drop_columns("geometry_projected")
    )
    mo.vstack(
        [
            mo.md(
                f"{len(_samples)} samples, from {_samples.t.min():%H:%M} to {_samples.t.max():%H:%M} UTC "
                f"on {_samples.t.min():%Y-%m-%d}, and the crossing they are matched against."
            ),
            _crossing.drop(columns="geometry"),
            lonboard.Map(
                [
                    lonboard.ScatterplotLayer.from_geopandas(
                        _samples,
                        get_fill_color=[120, 120, 120],
                        radius_units="pixels",
                        get_radius=3,
                    ),
                    lonboard.ScatterplotLayer.from_geopandas(
                        _crossing,
                        get_fill_color=[42, 120, 214],
                        radius_units="pixels",
                        get_radius=8,
                    ),
                ]
            ),
        ]
    )
    return


@app.cell
def _(case, crossing, lookout_session_crossings, mo, pa, samples):
    RADIUS_METRES = 250.0
    _steps = lookout_session_crossings.steps(
        case.session_id, samples, crossing, radius_metres=RADIUS_METRES
    )
    near = pa.table(_steps["near"])
    passes = pa.table(_steps["passes"])
    mo.vstack(
        [
            mo.md(
                f"{near.num_rows} samples lie within {RADIUS_METRES:.0f} m of the crossing. "
                f"They form {passes.num_rows} visits, so {passes.num_rows} passes, "
                f"and the case expects {case.expected_passes}."
            ),
            passes.to_pandas(),
        ]
    )
    return RADIUS_METRES, near, passes


@app.cell
def _():
    VISIT_COLOURS = ["#E69F00", "#009E73", "#CC79A7", "#0072B2"]
    return (VISIT_COLOURS,)


@app.cell
def _(VISIT_COLOURS, crossing, gpd, lonboard, near, samples):
    _near = gpd.GeoDataFrame.from_arrow(
        samples.drop_columns("geometry_projected")
    ).merge(near.to_pandas(), on="t")
    _crossing = gpd.GeoDataFrame.from_arrow(
        crossing.drop_columns("geometry_projected")
    )
    _colours = {
        visit: [int(colour[i : i + 2], 16) for i in (1, 3, 5)]
        for visit, colour in enumerate(VISIT_COLOURS)
    }
    lonboard.Map(
        [
            lonboard.ScatterplotLayer.from_geopandas(
                _near,
                get_fill_color=lonboard.colormap.apply_categorical_cmap(
                    _near["visit"], _colours
                ),
                radius_units="pixels",
                get_radius=5,
            ),
            lonboard.ScatterplotLayer.from_geopandas(
                _crossing,
                get_fill_color=[40, 40, 40],
                radius_units="pixels",
                get_radius=8,
            ),
        ]
    )
    return


@app.cell
def _(RADIUS_METRES, VISIT_COLOURS, alt, mo, near, passes):
    _near = near.to_pandas()
    _near["pass"] = _near["t"].isin(passes.to_pandas()["crossed_at"])
    _tooltip = [
        alt.Tooltip("t:T", title="Sample at", format="%H:%M:%S"),
        alt.Tooltip("distance_m:Q", title="Distance (m)", format=".0f"),
        alt.Tooltip("visit:N", title="Visit"),
        alt.Tooltip("pass:N", title="The visit's pass"),
    ]
    _base = alt.Chart().encode(
        x=alt.X("t:T", title="Time (UTC)", axis=alt.Axis(format="%H:%M:%S")),
        y=alt.Y(
            "distance_m:Q",
            title="Distance from the crossing (m)",
            scale=alt.Scale(domain=[0, RADIUS_METRES]),
        ),
        tooltip=_tooltip,
    )
    _samples = _base.mark_line(
        point=alt.OverlayMarkDef(size=64), strokeWidth=2
    ).encode(
        color=alt.Color(
            "visit:N",
            scale=alt.Scale(range=VISIT_COLOURS),
            legend=alt.Legend(title="Visit"),
        )
    )
    _passes = _base.transform_filter(alt.datum["pass"]).mark_point(
        shape="diamond", size=220, filled=True, color="#282828"
    )
    _radius = (
        alt.Chart()
        .mark_rule(strokeDash=[4, 4], color="#888888")
        .encode(y=alt.datum(RADIUS_METRES))
    )
    mo.ui.altair_chart(
        alt.layer(_samples, _passes, _radius, data=_near)
        .properties(width=260, height=240)
        .facet(column=alt.Column("visit:N", title="Visit"))
        .resolve_scale(x="independent")
        .properties(
            title="Near samples per visit, with the visit's pass as a diamond"
        )
    )
    return


if __name__ == "__main__":
    app.run()
