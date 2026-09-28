# /// script
# dependencies = [
#     "geopandas==1.1.4",
#     "lookout-medallion",
#     "pandas==3.0.6",
#     "pyarrow==25.0.0",
# ]
# requires-python = ">=3.13"
#
# # The store's own reader, so the crossings checked here are the rows the store holds.
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import sys
from pathlib import Path

import geopandas as gpd
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds

import lookout_medallion

HERE = Path(__file__).parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import crossing_checks

ROOT = Path(lookout_medallion.default_root())
CASES = HERE / "test_cases.geojson"


def extracts() -> pd.DataFrame:
    manifest = ds.dataset(ROOT / "bronze/extract_manifest", format="parquet").to_table()
    newest = manifest.to_pandas().sort_values("extracted_at").groupby("country").last()
    return newest[["extract_id", "min_lon", "min_lat", "max_lon", "max_lat"]]


def crossings_of(country: str) -> gpd.GeoDataFrame:
    rows = pa.table(
        lookout_medallion.query_silver(
            "SELECT crossing_id, rail_id, ST_AsBinary(geometry) AS wkb FROM water_crossing",
            country=country,
            root=str(ROOT),
        )
    ).to_pandas()
    return gpd.GeoDataFrame(
        rows.drop(columns=["wkb"]),
        geometry=gpd.GeoSeries.from_wkb(rows["wkb"], crs="EPSG:4326"),
    )


def rail_of(extract_id: str, bounds) -> gpd.GeoDataFrame:
    min_lon, min_lat, max_lon, max_lat = bounds
    segments = ds.dataset(
        ROOT / f"bronze/overture_extract/extract_id={extract_id}"
        "/theme=transportation/type=segment",
        format="parquet",
    ).to_table(
        columns={
            "id": ds.field("id"),
            "geometry": ds.field("geometry"),
            "xmin": ds.field("bbox", "xmin"),
            "xmax": ds.field("bbox", "xmax"),
            "ymin": ds.field("bbox", "ymin"),
            "ymax": ds.field("bbox", "ymax"),
        }
    ).to_pandas()
    near = segments[
        (segments["xmin"] <= max_lon)
        & (segments["xmax"] >= min_lon)
        & (segments["ymin"] <= max_lat)
        & (segments["ymax"] >= min_lat)
    ]
    return gpd.GeoDataFrame(
        near[["id"]],
        geometry=gpd.GeoSeries.from_wkb(near["geometry"], crs="EPSG:4326"),
    )


def country_of(taken: pd.DataFrame, bounds) -> str:
    lon, lat = (bounds[0] + bounds[2]) / 2, (bounds[1] + bounds[3]) / 2
    holds = taken[
        (taken["min_lon"] <= lon)
        & (taken["max_lon"] >= lon)
        & (taken["min_lat"] <= lat)
        & (taken["max_lat"] >= lat)
    ]
    return holds.index[0]


def main() -> int:
    cases = crossing_checks.load_cases(CASES)
    taken = extracts()
    crossings = {country: crossings_of(country) for country in taken.index}

    results = []
    for _, case in cases.iterrows():
        country = country_of(taken, case.geometry.bounds)
        rail = rail_of(taken.loc[country, "extract_id"], case.geometry.bounds)
        one = cases[cases["name"] == case["name"]]
        results.append(
            crossing_checks.run_cases(one, crossings[country], rail).assign(country=country)
        )

    table = pd.concat(results, ignore_index=True)
    print(table.to_string(index=False))
    return 0 if bool(table["pass"].all()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
