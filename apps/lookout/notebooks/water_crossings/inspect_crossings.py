# /// script
# dependencies = [
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion==0.1.0",
#     "marimo==0.25.0",
#     "numpy==2.5.3",
#     "pandas==3.0.6",
#     "pyarrow==25.0.0",
# ]
# requires-python = ">=3.13"
#
# # The store's own reader, so this notebook reads the store through the implementation that
# # wrote it. Built from the workspace by uv; see the crate's README.
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.0"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import geopandas as gpd
    import lonboard
    import numpy as np
    import pandas as pd
    import pyarrow as pa

    import lookout_medallion

    MEDALLION_ROOT = lookout_medallion.default_root()

    # A projected column is written in each country's own zone, so silver is read a country at a
    # time. See docs/medallion.md.
    COUNTRIES = lookout_medallion.countries_of(
        "water_crossing", root=str(MEDALLION_ROOT)
    )

    COUNTRIES
    return (
        COUNTRIES,
        MEDALLION_ROOT,
        gpd,
        lonboard,
        lookout_medallion,
        mo,
        np,
        pa,
        pd,
    )


@app.cell
def _(MEDALLION_ROOT, gpd, lookout_medallion, pa, pd):
    def with_geometry(rows: pd.DataFrame, wkb_column: str) -> gpd.GeoDataFrame:
        return gpd.GeoDataFrame(
            rows.drop(columns=[wkb_column]),
            geometry=gpd.GeoSeries.from_wkb(rows[wkb_column], crs="EPSG:4326"),
        )

    def silver(sql: str, country: str) -> gpd.GeoDataFrame:
        rows = pa.table(
            lookout_medallion.query_silver(
                sql, country=country, root=str(MEDALLION_ROOT)
            )
        ).to_pandas()
        return with_geometry(rows.assign(country=country), "wkb")

    def bronze(sql: str, **named) -> pd.DataFrame:
        return pa.table(
            lookout_medallion.query_bronze(
                sql, root=str(MEDALLION_ROOT), **named
            )
        ).to_pandas()

    def bronze_gdf(sql: str, **named) -> gpd.GeoDataFrame:
        return with_geometry(bronze(sql, **named), "geometry")

    return bronze, bronze_gdf, silver


@app.cell
def _(COUNTRIES, pd, silver):
    CROSSINGS = """
        SELECT crossing_id, water_id, water_class, water_subtype, track_id, rail_id,
               overlap_kind, overlap_m, total_overlap_m, merged_parts, frac,
               ST_AsBinary(geometry) AS wkb
        FROM water_crossing
    """

    crossings = pd.concat(
        [silver(CROSSINGS, country) for country in COUNTRIES],
        ignore_index=True,
    )
    crossings.groupby(["country", "overlap_kind"]).size().unstack(fill_value=0)
    return (crossings,)


@app.cell
def _(crossings, lonboard, mo, np):
    KIND_COLOUR = {"line": [11, 132, 165], "point": [246, 153, 30]}

    HOVER = [
        "water_class",
        "water_subtype",
        "overlap_kind",
        "overlap_m",
        "merged_parts",
    ]

    def country_map(country: str, radius: int = 3):
        of_country = crossings[crossings["country"] == country]
        shown = of_country[HOVER + ["geometry"]].copy()
        for column in HOVER:
            if shown[column].dtype == object:
                shown[column] = shown[column].astype("string")
        colours = np.asarray(
            [KIND_COLOUR[kind] for kind in of_country["overlap_kind"]],
            dtype="uint8",
        )

        return mo.vstack(
            [
                mo.md(
                    f"**{country}** — {len(of_country):,} crossings: "
                    + ", ".join(
                        f"{count:,} {kind}"
                        for kind, count in of_country["overlap_kind"]
                        .value_counts()
                        .items()
                    )
                ),
                lonboard.Map(
                    lonboard.ScatterplotLayer.from_geopandas(
                        shown,
                        get_fill_color=colours,
                        radius_units="pixels",
                        get_radius=radius,
                        radius_min_pixels=radius,
                        radius_max_pixels=radius,
                    ),
                    height=560,
                ),
            ]
        )

    return (country_map,)


@app.cell
def _(country_map):
    country_map("DE")
    return


@app.cell
def _(country_map):
    country_map("GB")
    return


@app.cell
def _(bronze):
    NEWEST_EXTRACTS = """
        WITH ranked AS (
            SELECT country, extract_id, min_lon, min_lat, max_lon, max_lat,
                   row_number() OVER (PARTITION BY country ORDER BY extracted_at DESC)
                       AS newest
            FROM extract_manifest
        )
        SELECT country, extract_id, min_lon, min_lat, max_lon, max_lat
        FROM ranked WHERE newest = 1 ORDER BY country
    """

    extracts = bronze(NEWEST_EXTRACTS).set_index("country")
    extracts
    return (extracts,)


@app.cell
def _(mo):
    import sys
    from pathlib import Path

    NOTEBOOK_DIR = Path(__file__).parent
    if str(NOTEBOOK_DIR) not in sys.path:
        sys.path.insert(0, str(NOTEBOOK_DIR))

    import crossing_checks
    import test_viz

    cases = crossing_checks.load_cases(NOTEBOOK_DIR / "test_cases.geojson")
    case_pick = mo.ui.dropdown(
        options=list(cases["name"]), value=cases["name"].iloc[0], label="case"
    )
    case_pick
    return case_pick, cases, test_viz


@app.cell
def _(bronze_gdf, case_pick, cases, crossings, extracts, test_viz):
    def country_whose_extract_covers(bounds) -> str:
        lon, lat = (bounds[0] + bounds[2]) / 2, (bounds[1] + bounds[3]) / 2
        covering = extracts[
            (extracts["min_lon"] <= lon)
            & (extracts["max_lon"] >= lon)
            & (extracts["min_lat"] <= lat)
            & (extracts["max_lat"] >= lat)
        ]
        return covering.index[0]

    def around(
        country: str, theme: str, type_: str, bounds, margin: float = 0.01
    ):
        min_lon, min_lat, max_lon, max_lat = bounds
        return bronze_gdf(
            """
            SELECT id, geometry FROM feature
            WHERE extract_id = $extract_id
              AND bbox['xmin'] <= $max_lon AND bbox['xmax'] >= $min_lon
              AND bbox['ymin'] <= $max_lat AND bbox['ymax'] >= $min_lat
            """,
            tables={"feature": {"theme": theme, "type": type_}},
            params={
                "extract_id": extracts.loc[country, "extract_id"],
                "min_lon": min_lon - margin,
                "max_lon": max_lon + margin,
                "min_lat": min_lat - margin,
                "max_lat": max_lat + margin,
            },
        )

    case = cases[cases["name"] == case_pick.value].iloc[0]
    case_country = country_whose_extract_covers(case.geometry.bounds)
    case_rail = around(
        case_country, "transportation", "segment", case.geometry.bounds
    )
    case_water = around(case_country, "base", "water", case.geometry.bounds)
    case_reps = crossings[crossings["country"] == case_country]

    test_viz.case_view(
        case,
        case_rail,
        case_reps,
        water_polys_gdf=case_water[
            case_water.geometry.geom_type.isin(["Polygon", "MultiPolygon"])
        ],
        water_lines_gdf=case_water[
            case_water.geometry.geom_type.isin(
                ["LineString", "MultiLineString"]
            )
        ],
    )
    return


if __name__ == "__main__":
    app.run()
