# /// script
# dependencies = [
#     "duckdb==1.5.5",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "marimo",
#     "pyarrow==25.0.0",
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

    return duckdb, gpd, lonboard


@app.cell
def _():
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

    def overture_glob(theme: str, type_: str) -> str:
        """S3 glob for one Overture theme/type partition (anonymous public bucket)."""
        return (
            f"s3://overturemaps-{S3_REGION}/release/{RELEASE}"
            f"/theme={theme}/type={type_}/*"
        )

    return EXCLUDED_RAIL_CLASSES, S3_REGION, TARGET_STATES, overture_glob


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
        SELECT s.id, s.class, s.geometry,
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
    _ = (water_count,)  # dataflow: run after water
    con.execute("""
        CREATE OR REPLACE TABLE crossings AS
        SELECT r.id AS rail_id, r.class AS rail_class,
               w.id AS water_id, w.subtype AS water_subtype,
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
def _(con, crossings_count):
    # Step 6: reduce each crossing to lat/lon centroid point(s), keeping source rail + water ids.
    # ST_Dump splits multi-part intersections (rail crossing the same water body at several places)
    # so each real crossing becomes its own point, rather than averaging them into a misleading mid-point.
    _ = (crossings_count,)  # dataflow: run after crossings
    con.execute("""
        CREATE OR REPLACE TABLE crossing_points AS
        WITH parts AS (
            SELECT rail_id, rail_class, water_id, water_subtype,
                   (UNNEST(ST_Dump(geom))).geom AS part
            FROM crossings
            WHERE NOT ST_IsEmpty(geom)
        )
        SELECT rail_id, rail_class, water_id, water_subtype,
               ST_Centroid(part) AS geom,
               ST_X(ST_Centroid(part)) AS lon,
               ST_Y(ST_Centroid(part)) AS lat
        FROM parts
        WHERE NOT ST_IsEmpty(part)
    """)
    crossing_points_count = con.execute(
        "SELECT count(*) FROM crossing_points"
    ).fetchone()[0]
    crossing_points_count
    return (crossing_points_count,)


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
def _(crossing_points_count, to_gdf):
    # GeoDataFrames for the cross-check map: rail lines, crossing points, and only the
    # water bodies that actually carry a crossing (split by geometry type for lonboard).
    _ = crossing_points_count  # dataflow: after the pipeline
    rail_gdf = to_gdf("SELECT id, class, geometry AS geom FROM rail")
    points_gdf = to_gdf(
        "SELECT rail_id, rail_class, water_id, water_subtype, lon, lat, geom FROM crossing_points"
    )
    _water_crossed = """
        SELECT id, subtype, geometry AS geom FROM water
        WHERE id IN (SELECT DISTINCT water_id FROM crossings)
    """
    water_lines_gdf = to_gdf(
        _water_crossed
        + " AND ST_GeometryType(geometry) IN ('LINESTRING','MULTILINESTRING')"
    )
    water_polys_gdf = to_gdf(
        _water_crossed
        + " AND ST_GeometryType(geometry) IN ('POLYGON','MULTIPOLYGON')"
    )
    len(rail_gdf), len(points_gdf), len(water_lines_gdf), len(water_polys_gdf)
    return points_gdf, rail_gdf, water_lines_gdf, water_polys_gdf


@app.cell
def _(lonboard, points_gdf, rail_gdf, water_lines_gdf, water_polys_gdf):
    # Step 7: cross-check map - crossing points (red) on rail (grey) over crossed water (blue).
    # NB geometry-only layers: pandas 3.0 + lonboard errors on object-dtype attribute columns,
    # so we pass just geometry (attribute tooltips can be restored once that is resolved).
    _water_poly_layer = lonboard.PolygonLayer.from_geopandas(
        water_polys_gdf[["geometry"]],
        get_fill_color=[40, 120, 220, 110],
        get_line_color=[40, 120, 220],
    )
    _water_line_layer = lonboard.PathLayer.from_geopandas(
        water_lines_gdf[["geometry"]],
        get_color=[40, 120, 220],
        width_min_pixels=1,
    )
    _rail_layer = lonboard.PathLayer.from_geopandas(
        rail_gdf[["geometry"]], get_color=[130, 130, 130], width_min_pixels=1
    )
    _points_layer = lonboard.ScatterplotLayer.from_geopandas(
        points_gdf[["geometry"]],
        get_fill_color=[220, 30, 30],
        radius_min_pixels=3,
        radius_max_pixels=8,
        get_radius=40,
    )
    crossings_map = lonboard.Map(
        [_water_poly_layer, _water_line_layer, _rail_layer, _points_layer]
    )
    crossings_map
    return


@app.cell
def _(con, crossing_points_count):
    # Export the pipeline outputs as GeoParquet (WKB, CRS 84) into data/water/v1/.
    # DuckDB spatial writes valid GeoParquet metadata directly via FORMAT PARQUET.
    # EXPORT_DIR is relative to the notebook dir (apps/lookout/notebooks/water_crossings).
    import os

    EXPORT_DIR = "../../data/water/v1"

    _ = crossing_points_count  # dataflow: run after the full pipeline
    os.makedirs(EXPORT_DIR, exist_ok=True)
    _tables = ["rail", "water", "crossings", "crossing_points"]
    for _t in _tables:
        con.execute(
            f"COPY {_t} TO '{EXPORT_DIR}/{_t}.parquet' (FORMAT PARQUET)"
        )

    export_manifest = {
        _t: os.path.getsize(f"{EXPORT_DIR}/{_t}.parquet") for _t in _tables
    }
    export_manifest
    return


if __name__ == "__main__":
    app.run()
