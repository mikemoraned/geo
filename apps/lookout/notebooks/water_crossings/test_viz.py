"""Per-test-case visualiser: focus a map on a test-case bbox and link out to the OvertureMaps
explorer, so a case's expected count can be eyeballed and corrected.

Mirrors `bbox_capture`'s convention — the module builds lonboard/marimo objects and the notebook
binds any UI element to a top-level global (marimo reactivity needs that). Given one test case
(a row from `crossing_checks.load_cases`) it renders, clipped to the bbox:

  * water (blue), rail (grey)
  * every raw rail∩water centroid (small grey) — the pre-filter candidates
  * the surviving V7 crossing reps (red, hover shows water_class / overlap_kind / frac)
  * the bbox outline (orange)

plus a summary line (expected vs found) with a link to explore.overturemaps.org centred on the
bbox at a fitted zoom. The explorer URL is `#<zoom>/<lat>/<lon>`.
"""

from __future__ import annotations

import math

import geopandas as gpd
import lonboard
import marimo as mo
from shapely.geometry import LineString, box

# nominal viewport used to turn a bbox into a (centre, zoom) — matches bbox_capture's map size
MAP_PX = (1000, 560)


def center(bounds):
    min_lon, min_lat, max_lon, max_lat = bounds
    return (min_lon + max_lon) / 2, (min_lat + max_lat) / 2


def fit_zoom(bounds, map_px=MAP_PX, pad=1.3):
    """Web-mercator zoom that fits `bounds` (with padding) into `map_px`. Inverse of
    bbox_capture.visible_bounds: 512*2**zoom px span 360°."""
    min_lon, min_lat, max_lon, max_lat = bounds
    _, clat = center(bounds)
    dlon = max((max_lon - min_lon) / 2, 1e-6) * pad
    dlat = max((max_lat - min_lat) / 2, 1e-6) * pad
    w, h = map_px
    zlon = math.log2(360.0 * (w / 2) / (512 * dlon))
    zlat = math.log2(
        360.0 * (h / 2) * max(math.cos(math.radians(clat)), 1e-6) / (512 * dlat)
    )
    return min(zlon, zlat)


def explorer_url(bounds, map_px=MAP_PX):
    """explore.overturemaps.org deep-link centred on the bbox at a fitted zoom."""
    clon, clat = center(bounds)
    return f"https://explore.overturemaps.org/#{fit_zoom(bounds, map_px):.2f}/{clat:.6f}/{clon:.6f}"


def _clip(gdf, bounds, margin=0.4):
    """Rows of `gdf` within the bbox expanded by `margin` (for a little context)."""
    min_lon, min_lat, max_lon, max_lat = bounds
    mlon = (max_lon - min_lon) * margin or 1e-4
    mlat = (max_lat - min_lat) * margin or 1e-4
    return gdf.cx[min_lon - mlon : max_lon + mlon, min_lat - mlat : max_lat + mlat]


def _scatter(gdf, color, radius, cols=None):
    g = gdf[(cols or []) + ["geometry"]].copy()
    for c in cols or []:
        g[c] = g[c].astype("string") if g[c].dtype == object else g[c]
    return lonboard.ScatterplotLayer.from_geopandas(
        g,
        get_fill_color=color,
        radius_units="pixels",
        get_radius=radius,
        radius_min_pixels=radius,
        radius_max_pixels=radius,
    )


def case_map(
    bounds,
    rail_gdf,
    reps_gdf,
    water_polys_gdf=None,
    water_lines_gdf=None,
    raw_gdf=None,
    map_height=MAP_PX[1],
):
    """Lonboard map clipped to `bounds`, view fitted to the bbox."""
    clon, clat = center(bounds)
    layers = []
    if water_polys_gdf is not None and len(_clip(water_polys_gdf, bounds)):
        layers.append(
            lonboard.PolygonLayer.from_geopandas(
                _clip(water_polys_gdf, bounds)[["geometry"]],
                get_fill_color=[40, 120, 220, 110],
                get_line_color=[40, 120, 220],
            )
        )
    if water_lines_gdf is not None and len(_clip(water_lines_gdf, bounds)):
        layers.append(
            lonboard.PathLayer.from_geopandas(
                _clip(water_lines_gdf, bounds)[["geometry"]],
                get_color=[40, 120, 220],
                width_min_pixels=1,
            )
        )
    rail_c = _clip(rail_gdf, bounds)
    if len(rail_c):
        layers.append(
            lonboard.PathLayer.from_geopandas(
                rail_c[["geometry"]], get_color=[140, 140, 140], width_min_pixels=1
            )
        )
    if raw_gdf is not None:
        raw_c = _clip(raw_gdf, bounds)
        if len(raw_c):
            layers.append(_scatter(raw_c, [170, 170, 170], 3))
    reps_c = _clip(reps_gdf, bounds)
    if len(reps_c):
        layers.append(
            _scatter(reps_c, [220, 30, 30], 6, ["water_class", "overlap_kind", "frac"])
        )
    outline = gpd.GeoDataFrame(geometry=[LineString(box(*bounds).exterior.coords)], crs="EPSG:4326")
    layers.append(
        lonboard.PathLayer.from_geopandas(
            outline, get_color=[255, 140, 0], width_min_pixels=2
        )
    )
    view_state = {"longitude": clon, "latitude": clat, "zoom": fit_zoom(bounds)}
    return lonboard.Map(layers, view_state=view_state, height=map_height)


def case_view(
    case,
    rail_gdf,
    reps_gdf,
    water_polys_gdf=None,
    water_lines_gdf=None,
    raw_gdf=None,
):
    """Summary line (expected vs found + explorer link) stacked over the focused map."""
    bounds = tuple(round(v, 6) for v in case.geometry.bounds)
    expected = int(case["expected_crossings"])
    found = int(reps_gdf.geometry.within(case.geometry).sum())
    mark = "✅" if found == expected else "❌"
    header = mo.md(
        f"### {case['name']} {mark}\n"
        f"expected **{expected}** · found **{found}** · "
        f"[open in OvertureMaps explorer]({explorer_url(bounds)})  \n"
        f"bbox `{list(bounds)}` · grey = raw rail∩water candidates, red = kept V7 crossings"
    )
    return mo.vstack(
        [
            header,
            case_map(bounds, rail_gdf, reps_gdf, water_polys_gdf, water_lines_gdf, raw_gdf),
        ]
    )
