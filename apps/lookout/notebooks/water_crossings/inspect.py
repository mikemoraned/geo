# /// script
# dependencies = [
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo",
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

__generated_with = "0.23.15"
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
            lookout_medallion.query_silver(sql, country=country, root=str(MEDALLION_ROOT))
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
        [silver(CROSSINGS, country) for country in COUNTRIES], ignore_index=True
    )
    crossings.groupby(["country", "overlap_kind"]).size().unstack(fill_value=0)
    return (crossings,)


@app.cell
def _(crossings, lonboard, mo, np):
    KIND_COLOUR = {"line": [11, 132, 165], "point": [246, 153, 30]}

    HOVER = ["water_class", "water_subtype", "overlap_kind", "overlap_m", "merged_parts"]


    def country_map(country: str, radius: int = 3):
        of_country = crossings[crossings["country"] == country]
        shown = of_country[HOVER + ["geometry"]].copy()
        for column in HOVER:
            if shown[column].dtype == object:
                shown[column] = shown[column].astype("string")
        colours = np.asarray(
            [KIND_COLOUR[kind] for kind in of_country["overlap_kind"]], dtype="uint8"
        )

        return mo.vstack([
            mo.md(
                f"**{country}** — {len(of_country):,} crossings: "
                + ", ".join(
                    f"{count:,} {kind}"
                    for kind, count in of_country["overlap_kind"].value_counts().items()
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
        ])

    return (country_map,)


@app.cell
def _(country_map):
    country_map("DE")
    return


@app.cell
def _(country_map):
    country_map("GB")
    return


if __name__ == "__main__":
    app.run()
