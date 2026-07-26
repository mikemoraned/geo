"""Validate water-crossing pipeline output against bbox test cases.

Test cases live in a GeoJSON `FeatureCollection` (see `test_cases.geojson`): each feature's
geometry is the bbox polygon and its `properties` carry the assertions (`name`,
`expected_crossings`, ...). Import this module into a notebook and run the cases against that
notebook's crossing representatives.
"""

from __future__ import annotations

import json

import geopandas as gpd
import pandas as pd


def add_case(path, name, bounds, expected_crossings, notes="") -> dict:
    """Append a bbox test case to a GeoJSON FeatureCollection file.

    `bounds` is (min_lon, min_lat, max_lon, max_lat); a rectangular Polygon feature is written
    with the assertions in its `properties`. Returns the appended feature.
    """
    min_lon, min_lat, max_lon, max_lat = (float(v) for v in bounds)
    feature = {
        "type": "Feature",
        "properties": {
            "name": name,
            "expected_crossings": int(expected_crossings),
            "notes": notes,
        },
        "geometry": {
            "type": "Polygon",
            "coordinates": [
                [
                    [min_lon, min_lat],
                    [max_lon, min_lat],
                    [max_lon, max_lat],
                    [min_lon, max_lat],
                    [min_lon, min_lat],
                ]
            ],
        },
    }
    with open(path) as f:
        collection = json.load(f)
    collection["features"].append(feature)
    with open(path, "w") as f:
        json.dump(collection, f, indent=2)
    return feature


def load_cases(path) -> gpd.GeoDataFrame:
    """Load bbox test cases from a GeoJSON FeatureCollection."""
    return gpd.read_file(path)


def run_cases(
    cases: gpd.GeoDataFrame,
    reps_gdf: gpd.GeoDataFrame,
    rail_gdf: gpd.GeoDataFrame,
) -> pd.DataFrame:
    """Check each case against the crossing representatives.

    A case passes when the number of representatives inside its bbox equals
    `expected_crossings`, and every such representative sits on a rail segment (`rail_id`)
    whose geometry intersects the bbox. Returns one result row per case.
    """
    rows = []
    for _, case in cases.iterrows():
        bbox = case.geometry
        inside = reps_gdf[reps_gdf.geometry.within(bbox)]
        segments_ok = bool(
            rail_gdf[rail_gdf["id"].isin(inside["rail_id"])].geometry.intersects(bbox).all()
        )
        expected = int(case["expected_crossings"])
        rows.append(
            {
                "name": case["name"],
                "expected": expected,
                "found": len(inside),
                "count_ok": len(inside) == expected,
                "segments_ok": segments_ok,
                "pass": len(inside) == expected and segments_ok,
            }
        )
    return pd.DataFrame(rows)
