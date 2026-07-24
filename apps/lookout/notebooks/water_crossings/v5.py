# /// script
# dependencies = [
#     "duckdb==1.5.5",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "marimo",
#     "matplotlib==3.11.1",
#     "numpy==2.5.1",
#     "pyarrow==25.0.0",
#     "scipy==1.18.0",
# ]
# requires-python = ">=3.13"
# ///

import marimo

__generated_with = "0.23.15"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import duckdb
    import geopandas as gpd
    import lonboard

    return duckdb, gpd, lonboard, mo


@app.cell
def _():
    import os as _os

    RELEASE = "2026-06-17.0"
    S3_REGION = "us-west-2"
    EXCLUDED_RAIL_CLASSES = ("tram",)

    # Overture "division" primary (local) names for the four target German states.
    # NB Overture uses local names: Hessen (not Hesse), Rheinland-Pfalz (not Rhineland-Palatinate).
    TARGET_STATES = (
        "Thüringen",
        "Hessen",
        "Baden-Württemberg",
        "Rheinland-Pfalz",
    )

    # Prefer a local Overture mirror if present (much faster than S3); else the public bucket.
    LOCAL_OVERTURE = f"/Volumes/PRO-G40/OvertureMaps/data/release/{RELEASE}"
    USE_LOCAL = _os.path.isdir(LOCAL_OVERTURE)

    # --- V2 tuning knobs -------------------------------------------------------
    PROJECTED_CRS = (
        "EPSG:25832"  # UTM 32N: metric, covers the four target states
    )
    MIN_CROSSING_M = (
        5.0  # drop areal-water crossings we'd pass too fast to see
    )
    # Point crossings (rail over a linear watercourse centreline) have no overlap length,
    # so keep them only for classes wide enough to notice from a train (tune as needed).
    SUBSTANTIAL_WATER_CLASSES = ("river", "canal", "fairway", "water")
    CITY_MIN_POPULATION = 50_000  # only label cities at least this big

    def overture_glob(theme: str, type_: str) -> str:
        """Glob for one Overture theme/type partition: local mirror if available, else S3."""
        if USE_LOCAL:
            return f"{LOCAL_OVERTURE}/theme={theme}/type={type_}/*"
        return f"s3://overturemaps-{S3_REGION}/release/{RELEASE}/theme={theme}/type={type_}/*"

    return (
        CITY_MIN_POPULATION,
        EXCLUDED_RAIL_CLASSES,
        MIN_CROSSING_M,
        PROJECTED_CRS,
        S3_REGION,
        SUBSTANTIAL_WATER_CLASSES,
        TARGET_STATES,
        overture_glob,
    )


@app.cell
def _(S3_REGION, duckdb):
    con = duckdb.connect()
    con.execute("INSTALL spatial; LOAD spatial;")
    con.execute("INSTALL httpfs; LOAD httpfs;")
    con.execute(f"SET s3_region = '{S3_REGION}';")
    con
    return (con,)


@app.cell
def _(TARGET_STATES, con, overture_glob):
    # Step 2: resolve the four states to their Overture region polygons; this is the
    # query window for the rail & water scans. `region_union` is the single clip geometry;
    # `region_bbox` is the pruning rectangle (also the dataflow handle later cells depend on).
    _names_sql = ", ".join(f"'{s}'" for s in TARGET_STATES)
    con.execute(f"""
        CREATE OR REPLACE TABLE regions AS
        SELECT names.primary AS name, id, geometry
        FROM read_parquet('{overture_glob("divisions", "division_area")}',
                          filename=true, hive_partitioning=1)
        WHERE subtype = 'region' AND country = 'DE'
          AND names.primary IN ({_names_sql})
    """)
    con.execute(
        "CREATE OR REPLACE TABLE region_union AS SELECT ST_Union_Agg(geometry) AS geom FROM regions"
    )

    region_names = [
        r[0]
        for r in con.execute(
            "SELECT name FROM regions ORDER BY name"
        ).fetchall()
    ]
    _b = con.execute("""
        SELECT MIN(ST_XMin(geometry)), MIN(ST_YMin(geometry)),
               MAX(ST_XMax(geometry)), MAX(ST_YMax(geometry))
        FROM regions
    """).fetchone()
    region_bbox = {"xmin": _b[0], "ymin": _b[1], "xmax": _b[2], "ymax": _b[3]}
    region_names, region_bbox
    return (region_bbox,)


@app.cell
def _(EXCLUDED_RAIL_CLASSES, con, overture_glob, region_bbox):
    # Step 3: rail extract - non-tram rail segments intersecting the region window.
    # bbox struct prefilter prunes row-groups; ST_Intersects against region_union clips precisely.
    # Envelope columns (min/max lon/lat) are kept for the bbox range-join in the crossings step.
    _bb = region_bbox  # dataflow dependency on the regions cell
    _excl = ", ".join(f"'{c}'" for c in EXCLUDED_RAIL_CLASSES)
    con.execute(f"""
        CREATE OR REPLACE TABLE rail AS
        SELECT s.id, s.class, s.connectors, s.geometry,
               s.bbox.xmin AS min_lon, s.bbox.xmax AS max_lon,
               s.bbox.ymin AS min_lat, s.bbox.ymax AS max_lat
        FROM read_parquet('{overture_glob("transportation", "segment")}',
                          filename=true, hive_partitioning=1) s
        WHERE s.subtype = 'rail'
          AND (s.class IS NULL OR s.class NOT IN ({_excl}))
          AND s.bbox.xmin <= {_bb["xmax"]} AND s.bbox.xmax >= {_bb["xmin"]}
          AND s.bbox.ymin <= {_bb["ymax"]} AND s.bbox.ymax >= {_bb["ymin"]}
          AND ST_Intersects(s.geometry, (SELECT geom FROM region_union))
    """)
    rail_count = con.execute("SELECT count(*) FROM rail").fetchone()[0]
    rail_count
    return (rail_count,)


@app.cell
def _(con, overture_glob, rail_count, region_bbox):
    # Step 4: water extract - Overture base/water whose bbox overlaps a rail segment's bbox.
    # The region-bbox prefilter prunes the global partition to ~1.3M candidates; the range-join
    # to rail envelopes keeps only water near a rail corridor. Envelope columns are retained so
    # the crossings step can range-join rail<->water cheaply.
    _ = (rail_count, region_bbox)  # dataflow: run after rail + regions
    _bb = region_bbox
    con.execute(f"""
        CREATE OR REPLACE TABLE water AS
        WITH cand AS (
            SELECT w.id, w.subtype, w.class, w.geometry,
                   w.bbox.xmin AS min_lon, w.bbox.xmax AS max_lon,
                   w.bbox.ymin AS min_lat, w.bbox.ymax AS max_lat
            FROM read_parquet('{overture_glob("base", "water")}',
                              filename=true, hive_partitioning=1) w
            WHERE w.bbox.xmin <= {_bb["xmax"]} AND w.bbox.xmax >= {_bb["xmin"]}
              AND w.bbox.ymin <= {_bb["ymax"]} AND w.bbox.ymax >= {_bb["ymin"]}
        )
        SELECT DISTINCT ON (c.id) c.id, c.subtype, c.class, c.geometry,
               c.min_lon, c.max_lon, c.min_lat, c.max_lat
        FROM cand c JOIN rail r
          ON r.min_lon <= c.max_lon AND r.max_lon >= c.min_lon
         AND r.min_lat <= c.max_lat AND r.max_lat >= c.min_lat
    """)
    water_count = con.execute("SELECT count(*) FROM water").fetchone()[0]
    water_count
    return (water_count,)


@app.cell
def _(con, water_count):
    # Step 5: rail x water intersections. bbox range-join keeps only overlapping pairs,
    # then ST_Intersects filters to true intersections; ST_Intersection is the overlap geometry
    # (a line where rail runs through areal water, a point where it crosses linear water).
    # water_class is carried so the V2 sizing step can keep point crossings by class.
    _ = (water_count,)  # dataflow: run after water
    con.execute("""
        CREATE OR REPLACE TABLE crossings AS
        SELECT r.id AS rail_id, r.class AS rail_class,
               w.id AS water_id, w.subtype AS water_subtype, w.class AS water_class,
               ST_Intersection(r.geometry, w.geometry) AS geom
        FROM rail r JOIN water w
          ON r.min_lon <= w.max_lon AND r.max_lon >= w.min_lon
         AND r.min_lat <= w.max_lat AND r.max_lat >= w.min_lat
         AND ST_Intersects(r.geometry, w.geometry)
    """)
    crossings_count = con.execute("SELECT count(*) FROM crossings").fetchone()[
        0
    ]
    crossings_count
    return (crossings_count,)


@app.cell
def _(
    MIN_CROSSING_M,
    PROJECTED_CRS,
    SUBSTANTIAL_WATER_CLASSES,
    con,
    crossings_count,
):
    # Step 6 (V2 + V4): dump each crossing into parts, size each part in a metric CRS, keep the ones
    # we'd notice, then V4-dedup: drop centreline POINT crossings that fall inside an areal water
    # polygon. A river stored as both a polygon and a centreline yields a redundant point there —
    # the rail also crosses the polygon as a line, which already represents the crossing.
    _ = (crossings_count,)  # dataflow: run after crossings
    _subst = ", ".join(f"'{c}'" for c in SUBSTANTIAL_WATER_CLASSES)
    con.execute(f"""
        CREATE OR REPLACE TABLE crossing_points AS
        WITH parts AS (
            SELECT rail_id, rail_class, water_id, water_subtype, water_class,
                   (UNNEST(ST_Dump(geom))).geom AS part
            FROM crossings
            WHERE NOT ST_IsEmpty(geom)
        ),
        sized AS (
            SELECT *,
                   CAST(ST_GeometryType(part) AS VARCHAR) AS part_type,
                   ST_Length(ST_Transform(part, 'EPSG:4326', '{PROJECTED_CRS}')) AS overlap_m,
                   ST_Centroid(part) AS cpt
            FROM parts
            WHERE NOT ST_IsEmpty(part)
        ),
        kept AS (
            SELECT row_number() OVER () AS rid, *
            FROM sized
            WHERE (part_type LIKE '%LINESTRING%' AND overlap_m > {MIN_CROSSING_M})
               OR (part_type LIKE '%POINT%' AND water_class IN ({_subst}))
        ),
        redundant AS (  -- V4: point crossings whose location lies inside an areal water polygon
            SELECT DISTINCT k.rid
            FROM kept k
            JOIN water wp
              ON k.part_type LIKE '%POINT%'
             AND CAST(ST_GeometryType(wp.geometry) AS VARCHAR) IN ('POLYGON', 'MULTIPOLYGON')
             AND wp.min_lon <= ST_X(k.cpt) AND wp.max_lon >= ST_X(k.cpt)
             AND wp.min_lat <= ST_Y(k.cpt) AND wp.max_lat >= ST_Y(k.cpt)
             AND ST_Contains(wp.geometry, k.cpt)
        )
        SELECT rail_id, rail_class, water_id, water_subtype, water_class,
               overlap_m,
               CASE WHEN part_type LIKE '%LINESTRING%' THEN 'line' ELSE 'point' END AS overlap_kind,
               cpt AS geom,
               ST_X(cpt) AS lon,
               ST_Y(cpt) AS lat
        FROM kept
        WHERE rid NOT IN (SELECT rid FROM redundant)
    """)
    crossing_points_count = con.execute(
        "SELECT count(*) FROM crossing_points"
    ).fetchone()[0]
    crossing_points_count
    return (crossing_points_count,)


@app.cell
def _(
    CITY_MIN_POPULATION,
    con,
    crossing_points_count,
    overture_glob,
    region_bbox,
):
    # City points for orientation on the map: Overture localities within the region above a
    # population cutoff. bbox prefilter prunes the partition; region_union clips precisely.
    _ = crossing_points_count  # dataflow: keep near the pipeline tail
    _bb = region_bbox
    con.execute(f"""
        CREATE OR REPLACE TABLE cities AS
        SELECT names.primary AS name, population,
               ST_X(geometry) AS lon, ST_Y(geometry) AS lat, geometry AS geom
        FROM read_parquet('{overture_glob("divisions", "division")}',
                          filename=true, hive_partitioning=1)
        WHERE country = 'DE' AND subtype = 'locality'
          AND population >= {CITY_MIN_POPULATION}
          AND bbox.xmin <= {_bb["xmax"]} AND bbox.xmax >= {_bb["xmin"]}
          AND bbox.ymin <= {_bb["ymax"]} AND bbox.ymax >= {_bb["ymin"]}
          AND ST_Intersects(geometry, (SELECT geom FROM region_union))
    """)
    cities_count = con.execute("SELECT count(*) FROM cities").fetchone()[0]
    cities_count
    return (cities_count,)


@app.cell
def _(con, gpd):
    def to_gdf(sql: str, geom_col: str = "geom") -> "gpd.GeoDataFrame":
        """Run a DuckDB query and return a GeoDataFrame (geometry via WKB, CRS 4326)."""
        df = con.execute(
            f"SELECT * EXCLUDE ({geom_col}), ST_AsWKB({geom_col}) AS _wkb FROM ({sql})"
        ).fetchdf()
        geom = gpd.GeoSeries.from_wkb(
            df.pop("_wkb").map(bytes), crs="EPSG:4326"
        )
        return gpd.GeoDataFrame(df, geometry=geom)

    return (to_gdf,)


@app.cell
def _(cities_count, crossing_points_count, to_gdf):
    # GeoDataFrames for the cross-check map: rail lines, sized crossing points, the water bodies
    # that actually carry a surviving crossing, and city markers for orientation.
    _ = (crossing_points_count, cities_count)  # dataflow: after the pipeline
    rail_gdf = to_gdf("SELECT id, class, geometry AS geom FROM rail")
    points_gdf = to_gdf(
        "SELECT rail_id, rail_class, water_id, water_subtype, water_class, overlap_m, overlap_kind, lon, lat, geom FROM crossing_points"
    )
    cities_gdf = to_gdf("SELECT name, population, lon, lat, geom FROM cities")
    _water_crossed = """
        SELECT id, subtype, geometry AS geom FROM water
        WHERE id IN (SELECT DISTINCT water_id FROM crossing_points)
    """
    water_lines_gdf = to_gdf(
        _water_crossed
        + " AND CAST(ST_GeometryType(geometry) AS VARCHAR) IN ('LINESTRING','MULTILINESTRING')"
    )
    water_polys_gdf = to_gdf(
        _water_crossed
        + " AND CAST(ST_GeometryType(geometry) AS VARCHAR) IN ('POLYGON','MULTIPOLYGON')"
    )
    (
        len(rail_gdf),
        len(points_gdf),
        len(cities_gdf),
        len(water_lines_gdf),
        len(water_polys_gdf),
    )
    return cities_gdf, points_gdf, rail_gdf, water_lines_gdf, water_polys_gdf


@app.cell
def _(crossing_points_count, mo, points_gdf):
    # Toggles for which overlap classes the map draws. Referencing these `.value`s in the
    # map cell makes it redraw reactively when a box is ticked.
    _ = crossing_points_count  # dataflow: place after the pipeline
    _n_line = int((points_gdf["overlap_kind"] == "line").sum())
    _n_point = int((points_gdf["overlap_kind"] == "point").sum())
    show_lines = mo.ui.checkbox(
        value=True,
        label=f"LINESTRING overlaps — areal water, track spans it ({_n_line})",
    )
    show_points = mo.ui.checkbox(
        value=True,
        label=f"POINT overlaps — linear watercourse centrelines ({_n_point})",
    )
    collapse_v5 = mo.ui.checkbox(
        value=True, label="collapse to one per (physical track, water body)"
    )
    merge_dist = mo.ui.slider(
        25,
        500,
        value=100,
        step=25,
        show_value=True,
        label="merge distance within track+water (m)",
    )
    mo.vstack(
        [
            mo.md("**Show overlap classes:**"),
            show_lines,
            show_points,
            mo.md("**V5 collapse (connector-component + distance):**"),
            collapse_v5,
            merge_dist,
        ]
    )
    return collapse_v5, merge_dist, show_lines, show_points


@app.cell
def _(con, crossing_points_count, lonboard, rail_gdf, to_gdf):
    # V5 · step 1/3 — physical tracks. Two crossing segments belong to the same physical track when
    # they share an Overture connector id; each connected component of that graph is one track.
    # Map: crossing segments coloured by track (grey = all rail context).
    _ = crossing_points_count  # dataflow: after the pipeline
    import numpy as _np
    import scipy.sparse as _ssp
    import scipy.sparse.csgraph as _csg

    def component_labels(n, edges):
        """Connected-component label (0..k-1) per node for an undirected n-node graph."""
        _e = _np.asarray(edges, dtype=int).reshape(-1, 2)
        _m = _ssp.coo_matrix(
            (_np.ones(len(_e)), (_e[:, 0], _e[:, 1])), shape=(n, n)
        )
        return _csg.connected_components(_m, directed=False)[1]

    def track_colors(ids):
        """RGB uint8 per component id — matplotlib's categorical tab20, cycled."""
        import numpy as np
        import matplotlib as mpl

        cmap = mpl.colormaps["tab20"]
        return (np.asarray([cmap(int(c) % 20)[:3] for c in ids]) * 255).astype(
            "uint8"
        )

    _rows = con.execute(
        "SELECT id, connectors FROM rail WHERE id IN (SELECT DISTINCT rail_id FROM crossing_points)"
    ).fetchall()
    _seg_ix = {sid: i for i, (sid, _) in enumerate(_rows)}
    _seen, _seg_edges = {}, []
    for _sid, _conns in _rows:
        for _c in _conns or []:
            _cid = _c["connector_id"]
            if _cid in _seen:
                _seg_edges.append((_seg_ix[_sid], _seen[_cid]))
            else:
                _seen[_cid] = _seg_ix[_sid]
    component = {
        sid: int(lbl)
        for sid, lbl in zip(
            _seg_ix, component_labels(len(_seg_ix), _seg_edges)
        )
    }

    seg_components_gdf = to_gdf(
        "SELECT id, geometry AS geom FROM rail WHERE id IN (SELECT DISTINCT rail_id FROM crossing_points)"
    )
    seg_components_gdf["component_id"] = seg_components_gdf["id"].map(
        component
    )
    _colors = track_colors(seg_components_gdf["component_id"])
    lonboard.Map(
        [
            lonboard.PathLayer.from_geopandas(
                rail_gdf[["geometry"]],
                get_color=[220, 220, 220],
                width_min_pixels=1,
            ),
            lonboard.PathLayer.from_geopandas(
                seg_components_gdf[["geometry"]],
                get_color=_colors,
                width_min_pixels=3,
            ),
        ]
    )
    return component, component_labels, track_colors


@app.cell
def _(PROJECTED_CRS, component, lonboard, points_gdf, rail_gdf, track_colors):
    # V5 · step 2/3 — label each kept crossing part with its physical track (component) and compute
    # metric coordinates for the distance step. Map: crossing points in the same track colours.
    import numpy as _np

    parts_gdf = points_gdf.reset_index(drop=True).copy()
    parts_gdf["component_id"] = parts_gdf["rail_id"].map(component)
    _proj = parts_gdf.to_crs(PROJECTED_CRS)
    parts_xy = _np.column_stack(
        [_proj.geometry.x.to_numpy(), _proj.geometry.y.to_numpy()]
    )

    _colors = track_colors(parts_gdf["component_id"])
    lonboard.Map(
        [
            lonboard.PathLayer.from_geopandas(
                rail_gdf[["geometry"]],
                get_color=[220, 220, 220],
                width_min_pixels=1,
            ),
            lonboard.ScatterplotLayer.from_geopandas(
                parts_gdf[["geometry"]],
                get_fill_color=_colors,
                radius_units="pixels",
                get_radius=4,
                radius_min_pixels=4,
                radius_max_pixels=4,
            ),
        ]
    )
    return parts_gdf, parts_xy


@app.cell
def _(
    component_labels,
    lonboard,
    merge_dist,
    parts_gdf,
    parts_xy,
    rail_gdf,
    track_colors,
):
    # V5 · step 3/3 — final crossings. Link parts that share a (track, water body) and are within
    # `merge_dist` metres (scipy cKDTree → connected components); keep one representative per group
    # (largest overlap). Map: faint grey = input parts; coloured dots = reps, sized by parts merged.
    import numpy as _np
    import scipy.spatial as _sps

    _D = float(merge_dist.value)
    _key = (
        parts_gdf["component_id"].astype(str)
        + "|"
        + parts_gdf["water_id"].astype(str)
    ).to_numpy()
    _near = _sps.cKDTree(parts_xy).query_pairs(_D, output_type="ndarray")
    _edges = (
        _near[_key[_near[:, 0]] == _key[_near[:, 1]]]
        if len(_near)
        else _np.empty((0, 2), int)
    )
    _clustered = parts_gdf.assign(
        _cluster=component_labels(len(parts_gdf), _edges)
    )

    _stats = (
        _clustered.groupby("_cluster")["overlap_m"]
        .agg(["size", "sum"])
        .rename(columns={"size": "merged_parts", "sum": "total_overlap_m"})
        .reset_index()
    )
    reps_v5_gdf = (
        _clustered.loc[_clustered.groupby("_cluster")["overlap_m"].idxmax()]
        .merge(_stats, on="_cluster")
        .drop(columns=["_cluster"])
    )

    _colors = track_colors(reps_v5_gdf["component_id"])
    _sizes = (reps_v5_gdf["merged_parts"].to_numpy() * 2 + 3).astype("float32")
    lonboard.Map(
        [
            lonboard.PathLayer.from_geopandas(
                rail_gdf[["geometry"]],
                get_color=[220, 220, 220],
                width_min_pixels=1,
            ),
            lonboard.ScatterplotLayer.from_geopandas(
                parts_gdf[["geometry"]],
                get_fill_color=[200, 200, 200],
                radius_units="pixels",
                get_radius=2,
                radius_min_pixels=2,
                radius_max_pixels=2,
            ),
            lonboard.ScatterplotLayer.from_geopandas(
                reps_v5_gdf[["geometry"]],
                get_fill_color=_colors,
                stroked=True,
                get_line_color=[0, 0, 0],
                line_width_min_pixels=1,
                radius_units="pixels",
                get_radius=_sizes,
                radius_min_pixels=3,
                radius_max_pixels=14,
            ),
        ]
    )
    return (reps_v5_gdf,)


@app.cell
def _(
    cities_gdf,
    collapse_v5,
    lonboard,
    points_gdf,
    rail_gdf,
    reps_v5_gdf,
    show_lines,
    show_points,
    water_lines_gdf,
    water_polys_gdf,
):
    # Step 7 (V2): cross-check map.
    #   - water (blue) + rail (grey) context
    #   - each shown crossing: a 3px centre dot, plus an open circle whose radius (metres)
    #     is proportional to the spanned overlap length (point crossings have length 0 -> no circle)
    #   - the show_lines / show_points toggles pick which overlap classes are drawn
    #   - city markers (population >= cutoff); hover shows the name (lonboard 0.16 has no TextLayer)
    _kinds = [
        k
        for k, on in (("line", show_lines.value), ("point", show_points.value))
        if on
    ]
    _src = reps_v5_gdf if collapse_v5.value else points_gdf
    _pts = _src[_src["overlap_kind"].isin(_kinds)]

    _layers = [
        lonboard.PolygonLayer.from_geopandas(
            water_polys_gdf[["geometry"]],
            get_fill_color=[40, 120, 220, 110],
            get_line_color=[40, 120, 220],
        ),
        lonboard.PathLayer.from_geopandas(
            water_lines_gdf[["geometry"]],
            get_color=[40, 120, 220],
            width_min_pixels=1,
        ),
        lonboard.PathLayer.from_geopandas(
            rail_gdf[["geometry"]],
            get_color=[130, 130, 130],
            width_min_pixels=1,
        ),
    ]

    if len(_pts):
        # open circle: radius in metres ~ half the spanned overlap length, drawn from the centroid
        _layers.append(
            lonboard.ScatterplotLayer.from_geopandas(
                _pts[["geometry"]],
                stroked=True,
                filled=False,
                get_line_color=[220, 30, 30, 180],
                line_width_min_pixels=1,
                radius_units="meters",
                get_radius=(
                    _pts["total_overlap_m"]
                    if "total_overlap_m" in _pts.columns
                    else _pts["overlap_m"]
                ).to_numpy()
                / 2.0,
                radius_min_pixels=0,
            )
        )
        # tiny centre dot (hover shows the crossing size, kind + water class)
        _centre_gdf = _pts[
            [
                "overlap_m",
                "overlap_kind",
                "water_class",
                "water_subtype",
                "geometry",
            ]
        ].copy()
        for _c in ("overlap_kind", "water_class", "water_subtype"):
            _centre_gdf[_c] = _centre_gdf[_c].astype("string")
        _layers.append(
            lonboard.ScatterplotLayer.from_geopandas(
                _centre_gdf,
                get_fill_color=[220, 30, 30],
                stroked=False,
                radius_units="pixels",
                get_radius=3,
                radius_min_pixels=3,
                radius_max_pixels=3,
            )
        )

    # city markers (hover shows the name)
    _cities_named = cities_gdf[["name", "population", "geometry"]].copy()
    _cities_named["name"] = _cities_named["name"].astype("string")
    _layers.append(
        lonboard.ScatterplotLayer.from_geopandas(
            _cities_named,
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

    crossings_map = lonboard.Map(_layers)
    crossings_map
    return


@app.cell
def _(con, crossing_points_count, reps_v5_gdf):
    # Export the pipeline outputs as GeoParquet (WKB, CRS 84) under data/water/<notebook stem>/.
    # DuckDB spatial writes valid GeoParquet metadata directly via FORMAT PARQUET.
    # EXPORT_DIR is derived from this notebook's own filename (v1, v2, ...) so slices
    # don't clobber each other's outputs.
    import os
    from pathlib import Path

    _nb = Path(__file__)
    EXPORT_DIR = (_nb.parent / "../../data/water" / _nb.stem).resolve()

    _ = (
        crossing_points_count,
        reps_v5_gdf,
    )  # dataflow: after the pipeline + V5 reduction
    os.makedirs(EXPORT_DIR, exist_ok=True)
    _tables = ["rail", "water", "crossings", "crossing_points"]
    for _t in _tables:
        _path = str(EXPORT_DIR / f"{_t}.parquet")
        con.execute(f"COPY {_t} TO '{_path}' (FORMAT PARQUET)")

    reps_v5_gdf.to_parquet(EXPORT_DIR / "crossing_reps.parquet")

    export_manifest = {
        _t: os.path.getsize(EXPORT_DIR / f"{_t}.parquet") for _t in _tables
    }
    export_manifest["crossing_reps"] = os.path.getsize(
        EXPORT_DIR / "crossing_reps.parquet"
    )
    export_manifest
    return


@app.cell
def _(rail_gdf, reps_v5_gdf):
    # Test cases: check the V5 output against the bbox cases in test_cases.geojson, using the shared
    # `crossing_checks` module imported from this notebook's directory.
    import sys as _sys
    from pathlib import Path as _Path

    _here = _Path(__file__).parent
    if str(_here) not in _sys.path:
        _sys.path.insert(0, str(_here))
    import crossing_checks as _cc

    test_cases = _cc.load_cases(_here / "test_cases.geojson")
    test_results = _cc.run_cases(test_cases, reps_v5_gdf, rail_gdf)
    test_results
    return


if __name__ == "__main__":
    app.run()
