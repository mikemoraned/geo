# /// script
# dependencies = [
#     "duckdb==1.5.5",
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
# # The store's own reader, so this notebook reads silver through the implementation that wrote
# # it. Built from the workspace by uv; see the crate's README.
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
    COUNTRIES = ("DE", "GB")

    MEDALLION_ROOT
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
def _(COUNTRIES, MEDALLION_ROOT, gpd, lookout_medallion, pa, pd):
    def silver(sql: str, country: str) -> gpd.GeoDataFrame:
        rows = pa.table(
            lookout_medallion.query_silver(
                sql, country=country, root=str(MEDALLION_ROOT)
            )
        ).to_pandas()
        return gpd.GeoDataFrame(
            rows.drop(columns=["wkb"]).assign(country=country),
            geometry=gpd.GeoSeries.from_wkb(rows["wkb"], crs="EPSG:4326"),
        )

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
def _(MEDALLION_ROOT):
    import duckdb

    con = duckdb.connect()
    con.execute("INSTALL spatial; LOAD spatial;")

    # The newest extract taken for each country, which is the one the crossings were derived from.
    extracts = (
        con.execute(f"""
        SELECT country, extract_id, min_lon, min_lat, max_lon, max_lat
        FROM read_parquet('{MEDALLION_ROOT}/bronze/extract_manifest/*.parquet')
        QUALIFY row_number() OVER (PARTITION BY country ORDER BY extracted_at DESC) = 1
        ORDER BY country
    """)
        .fetchdf()
        .set_index("country")
    )
    extracts
    return con, extracts


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
def _(
    MEDALLION_ROOT,
    case_pick,
    cases,
    con,
    crossings,
    extracts,
    gpd,
    test_viz,
):
    def bronze_gdf(sql: str) -> gpd.GeoDataFrame:
        rows = con.execute(
            f"SELECT * EXCLUDE (geom), ST_AsWKB(geom) AS wkb FROM ({sql})"
        ).fetchdf()
        return gpd.GeoDataFrame(
            rows.drop(columns=["wkb"]),
            geometry=gpd.GeoSeries.from_wkb(
                rows["wkb"].map(bytes), crs="EPSG:4326"
            ),
        )

    def extract_glob(country: str, theme: str, type_: str) -> str:
        extract_id = extracts.loc[country, "extract_id"]
        return (
            f"{MEDALLION_ROOT}/bronze/overture_extract/extract_id={extract_id}"
            f"/theme={theme}/type={type_}/*.parquet"
        )

    def country_of(bounds) -> str:
        """The country whose extract window holds the middle of `bounds`."""
        lon, lat = (bounds[0] + bounds[2]) / 2, (bounds[1] + bounds[3]) / 2
        holding = extracts[
            (extracts["min_lon"] <= lon)
            & (extracts["max_lon"] >= lon)
            & (extracts["min_lat"] <= lat)
            & (extracts["max_lat"] >= lat)
        ]
        return holding.index[0]

    def around(
        country: str, theme: str, type_: str, bounds, margin: float = 0.01
    ):
        min_lon, min_lat, max_lon, max_lat = bounds
        return f"""
            SELECT id, geometry AS geom
            FROM read_parquet('{extract_glob(country, theme, type_)}')
            WHERE bbox.xmin <= {max_lon + margin} AND bbox.xmax >= {min_lon - margin}
              AND bbox.ymin <= {max_lat + margin} AND bbox.ymax >= {min_lat - margin}
        """

    case = cases[cases["name"] == case_pick.value].iloc[0]
    case_country = country_of(case.geometry.bounds)
    case_rail = bronze_gdf(
        around(case_country, "transportation", "segment", case.geometry.bounds)
    )
    case_water = bronze_gdf(
        around(case_country, "base", "water", case.geometry.bounds)
    )
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
