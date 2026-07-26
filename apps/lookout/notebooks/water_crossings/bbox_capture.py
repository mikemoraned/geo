"""Reusable interactive bbox-capture widget for building test cases (see `crossing_checks`).

Because a marimo UI element's value can't be read in the cell that created it — and UI reactivity
only works when the elements are top-level notebook globals (not object attributes) — this module
*builds and returns* the elements and the notebook binds them to globals across two cells:

    # cell 1 — build + display
    import bbox_capture
    capture_map, case_name, case_expected, refresh, append = bbox_capture.make_capture(
        reps_gdf, rail_gdf, cities_gdf
    )
    bbox_capture.controls(capture_map, case_name, case_expected, refresh, append)

    # cell 2 — preview + append (references the same globals, so it stays reactive)
    bbox_capture.result(
        capture_map, case_name, case_expected, refresh, append, reps_gdf, "test_cases.geojson"
    )

`reps_gdf` must have `lon` / `lat` columns and point geometry; a case's bbox is the map's visible
area, derived from the map's synced `view_state`.
"""

from __future__ import annotations

import math

import lonboard
import marimo as mo

import crossing_checks

# nominal capture-map pixel size, used to turn (centre, zoom) into a visible-area bbox
MAP_PX = (1000, 560)


def make_capture(reps_gdf, rail_gdf=None, cities_gdf=None, map_height=MAP_PX[1]):
    """Build the capture UI. Returns (capture_map, case_name, case_expected, refresh, append) —
    bind these to top-level globals in the notebook so marimo keeps them reactive."""
    layers = []
    if rail_gdf is not None:
        layers.append(
            lonboard.PathLayer.from_geopandas(
                rail_gdf[["geometry"]], get_color=[150, 150, 150], width_min_pixels=1
            )
        )
    layers.append(
        lonboard.ScatterplotLayer.from_geopandas(
            reps_gdf[["geometry"]],
            get_fill_color=[220, 30, 30],
            radius_units="pixels",
            get_radius=4,
            radius_min_pixels=4,
            radius_max_pixels=4,
        )
    )
    if cities_gdf is not None:
        named = cities_gdf[["name", "geometry"]].copy()
        named["name"] = named["name"].astype("string")
        layers.append(
            lonboard.ScatterplotLayer.from_geopandas(
                named,
                get_fill_color=[30, 30, 30, 220],
                stroked=True,
                get_line_color=[255, 255, 255],
                line_width_min_pixels=1,
                radius_units="pixels",
                get_radius=5,
                radius_min_pixels=5,
                radius_max_pixels=5,
            )
        )
    capture_map = mo.ui.anywidget(lonboard.Map(layers, height=map_height))
    case_name = mo.ui.text(placeholder="e.g. Koblenz Rhine bridge", label="case name")
    case_expected = mo.ui.number(0, 200, value=1, label="expected_crossings")
    refresh = mo.ui.run_button(label="Refresh from view")
    append = mo.ui.run_button(label="Append visible area")
    return capture_map, case_name, case_expected, refresh, append


def controls(capture_map, case_name, case_expected, refresh, append):
    """Layout for the capture widgets — display this in the cell that created them."""
    return mo.vstack(
        [capture_map, mo.hstack([case_name, case_expected, refresh, append])]
    )


def visible_bounds(capture_map, map_px=MAP_PX):
    """(min_lon, min_lat, max_lon, max_lat) of the map's visible area, from its synced view_state,
    or None if the map hasn't reported a view yet. deck.gl web-mercator: 512*2**zoom px span 360°."""
    vs = capture_map.value.get("view_state") or {}
    zoom, clat, clon = vs.get("zoom"), vs.get("latitude"), vs.get("longitude")
    if zoom is None:
        return None
    width, height = map_px
    deg_per_px = 360.0 / (512 * 2**zoom)
    dlon = (width / 2) * deg_per_px
    dlat = (height / 2) * deg_per_px * math.cos(math.radians(clat))
    return tuple(round(v, 5) for v in (clon - dlon, clat - dlat, clon + dlon, clat + dlat))


def result(capture_map, case_name, case_expected, refresh, append, reps_gdf, cases_path):
    """Preview the visible-area bbox + in-box rep count, and append the case when `append` is
    clicked. Call this in a *different* cell from `make_capture`; returns a marimo markdown status."""
    refresh.value  # take a dependency so "Refresh from view" re-runs this
    bounds = visible_bounds(capture_map)
    if bounds is None:
        return mo.md(
            "*Pan/zoom the map so the target fills the view, then click Refresh from view.*"
        )
    min_lon, min_lat, max_lon, max_lat = bounds
    found = int(
        (
            (reps_gdf.lon >= min_lon)
            & (reps_gdf.lon <= max_lon)
            & (reps_gdf.lat >= min_lat)
            & (reps_gdf.lat <= max_lat)
        ).sum()
    )
    if append.value and case_name.value:
        crossing_checks.add_case(cases_path, case_name.value, bounds, case_expected.value)
        return mo.md(
            f"✅ Appended **{case_name.value}** — visible area `{list(bounds)}` "
            f"({found} crossings). Re-run the test-cases cell to include it."
        )
    return mo.md(
        f"**Visible area** `{list(bounds)}` — **{found}** crossings inside. "
        "Frame the target, name it, and click Append."
    )
