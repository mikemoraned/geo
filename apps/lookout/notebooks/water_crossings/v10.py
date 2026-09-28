# /// script
# dependencies = [
#     "duckdb==1.5.5",
#     "geopandas==1.1.4",
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo==0.25.0",
#     "matplotlib==3.11.1",
#     "numpy==2.5.1",
#     "pyarrow==25.0.0",
#     "scipy==1.18.0",
# ]
# requires-python = ">=3.13"
#
# # The store's own writer, so this notebook's output is silver in exactly the form the
# # rust derivations write it. Built from the workspace by uv; see the crate's README.
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.0"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import duckdb
    import geopandas as gpd
    import lonboard
    import lookout_medallion

    return duckdb, gpd, lonboard, lookout_medallion, mo


@app.cell
def _(lookout_medallion):
    # The bronze Overture extract this notebook reads for each country, pinned. A rerun is meant
    # to see exactly what the last run saw, so moving to a newer extract is a deliberate edit here
    # rather than something that happens on its own. `just bronze-extract` writes them; the release
    # each covers and the window it was restricted to are in the manifest, read below. One run
    # covers every country here and writes them in one go, which is what keeps the crossing ids
    # unique across the dataset.
    EXTRACTS = {
        "DE": "20260804T152143Z",
        "GB": "20260927T172559Z",
    }

    # The medallion store in the repo. Asked of the store's own writer rather than worked out
    # here, so what this notebook reads with duckdb is the store its writes land in.
    MEDALLION_ROOT = lookout_medallion.default_root()

    EXCLUDED_RAIL_CLASSES = ("tram",)

    # --- V2 tuning knobs -------------------------------------------------------
    MIN_CROSSING_M = (
        5.0  # drop areal-water crossings we'd pass too fast to see
    )
    # Point crossings (rail over a linear watercourse centreline) have no overlap length,
    # so keep them only for classes wide enough to notice from a train (tune as needed).
    SUBSTANTIAL_WATER_CLASSES = ("river", "canal", "fairway", "water")
    CITY_MIN_POPULATION = 50_000  # only label cities at least this big

    # V7: rail_flag values that mean no train would see the water at that point along the segment,
    # so the crossing should be dropped. Two reasons: view blocked (is_tunnel / is_covered) or no
    # train runs there (is_abandoned / is_disused / is_under_construction). is_bridge is the
    # opposite (an elevated, visible crossing over water) and is deliberately NOT excluded.
    EXCLUDE_RAIL_FLAGS = (
        "is_tunnel",
        "is_covered",
        "is_abandoned",
        "is_disused",
        "is_under_construction",
    )

    def extract_globs(theme: str, type_: str) -> str:
        """SQL list of that theme and type across every country's extract.

        A read of it carries the `extract_id` the extract wrote into every row, which the
        `extracts` table below turns into the country whose extract it came out of.
        """
        paths = [
            f"{MEDALLION_ROOT}/bronze/overture_extract/extract_id={extract_id}"
            f"/theme={theme}/type={type_}/*.parquet"
            for extract_id in EXTRACTS.values()
        ]
        return "[" + ", ".join(f"'{path}'" for path in paths) + "]"

    return (
        CITY_MIN_POPULATION,
        EXCLUDED_RAIL_CLASSES,
        EXCLUDE_RAIL_FLAGS,
        EXTRACTS,
        MEDALLION_ROOT,
        MIN_CROSSING_M,
        SUBSTANTIAL_WATER_CLASSES,
        extract_globs,
    )


@app.cell
def _(duckdb):
    con = duckdb.connect()
    con.execute("INSTALL spatial; LOAD spatial;")
    con
    return (con,)


@app.cell
def _(EXTRACTS, con, lookout_medallion):
    # Which country's extract a row came out of, and what names that country upstream. A row's
    # own `country` column says where the feature is, which for rail inside a coastal window is
    # sometimes a neighbour; this says whose extract holds it. The division id is what the
    # country's own areas carry, and it is read rather than written here so the notebook and the
    # store cannot disagree about which entity a country is.
    _values = ", ".join(
        f"('{country}', '{extract_id}', '{lookout_medallion.division_id(country)}')"
        for country, extract_id in EXTRACTS.items()
    )
    con.execute(f"""
        CREATE OR REPLACE TABLE extracts AS
        SELECT * FROM (VALUES {_values}) AS t(country, extract_id, division_id)
    """)
    extracts = con.execute("SELECT * FROM extracts ORDER BY country").fetchdf()
    extracts
    return (extracts,)


@app.cell
def _(EXTRACTS, MEDALLION_ROOT, con):
    # What the pinned extracts actually are: the Overture release each was taken from, when it
    # was taken, and the window it was restricted to. Read here so the notebook states its inputs
    # rather than leaving them implicit in a path, and so a missing extract fails loudly at the
    # top instead of as an empty table further down. The countries need not share a release; what
    # has to hold is that each extract was taken for the country it is pinned under.
    _ids = ", ".join(f"'{extract_id}'" for extract_id in EXTRACTS.values())
    extract_manifest = con.execute(f"""
        SELECT extract_id, extracted_at, release, country,
               min_lon, min_lat, max_lon, max_lat
        FROM read_parquet('{MEDALLION_ROOT}/bronze/extract_manifest/*.parquet')
        WHERE extract_id IN ({_ids})
        ORDER BY country
    """).fetchdf()
    assert len(extract_manifest) == len(EXTRACTS), (
        f"expected one manifest row per pinned extract {sorted(EXTRACTS.values())}, "
        f"found {len(extract_manifest)}"
    )
    assert dict(
        zip(extract_manifest["extract_id"], extract_manifest["country"])
    ) == {extract_id: country for country, extract_id in EXTRACTS.items()}, (
        "a pinned extract was taken for a different country than it is pinned under"
    )
    extract_manifest
    return


@app.cell
def _(con, extract_globs, extracts):
    # Step 2 (V6): the query window is each country's own areas — the land and the territorial
    # waters its division carries. `region_union` is the clip geometry per country;
    # `region_bbox` is the pruning rectangle per country, and `region_areas` the dataflow handle
    # later cells depend on. An extract restricts to its country by bbox, which reaches over the
    # border and out to sea; the union clips precisely, so a neighbour's rivers and rail are
    # dropped here rather than upstream.
    _ = extracts  # dataflow: the country of each extract is read below
    con.execute(f"""
        CREATE OR REPLACE TABLE regions AS
        SELECT e.country, d.names.primary AS name, d.id, d.class, d.geometry
        FROM read_parquet({extract_globs("divisions", "division_area")}) d
        JOIN extracts e
          ON e.extract_id = d.extract_id AND e.division_id = d.division_id
    """)
    con.execute("""
        CREATE OR REPLACE TABLE region_union AS
        SELECT country, ST_Union_Agg(geometry) AS geom FROM regions GROUP BY country
    """)
    con.execute("""
        CREATE OR REPLACE TABLE region_bbox AS
        SELECT country,
               MIN(ST_XMin(geometry)) AS xmin, MIN(ST_YMin(geometry)) AS ymin,
               MAX(ST_XMax(geometry)) AS xmax, MAX(ST_YMax(geometry)) AS ymax
        FROM regions GROUP BY country
    """)

    region_areas = con.execute(
        "SELECT country, name, class, xmin, ymin, xmax, ymax FROM regions"
        " JOIN region_bbox USING (country) ORDER BY country, class"
    ).fetchdf()
    assert len(region_areas) == 2 * con.execute(
        "SELECT count(*) FROM extracts"
    ).fetchone()[0], (
        "each country's division carries two areas, its land and its territorial waters"
    )
    region_areas
    return (region_areas,)


@app.cell
def _(EXCLUDED_RAIL_CLASSES, con, extract_globs, region_areas):
    # Step 3: rail extract - non-tram rail segments intersecting their own country's areas.
    # bbox struct prefilter prunes row-groups; ST_Intersects against that country's region_union
    # clips precisely. Envelope columns (min/max lon/lat) are kept for the bbox range-join in the
    # crossings step. The extract already holds only non-tram rail; the filters stay so this cell
    # states what it needs rather than depending on how the extract was taken.
    _ = region_areas  # dataflow dependency on the regions cell
    _excl = ", ".join(f"'{c}'" for c in EXCLUDED_RAIL_CLASSES)
    con.execute(f"""
        CREATE OR REPLACE TABLE rail AS
        SELECT e.country, s.id, s.class, s.connectors, s.rail_flags, s.geometry,
               s.bbox.xmin AS min_lon, s.bbox.xmax AS max_lon,
               s.bbox.ymin AS min_lat, s.bbox.ymax AS max_lat
        FROM read_parquet({extract_globs("transportation", "segment")}) s
        JOIN extracts e ON e.extract_id = s.extract_id
        JOIN region_bbox b ON b.country = e.country
        JOIN region_union u ON u.country = e.country
        WHERE s.subtype = 'rail'
          AND (s.class IS NULL OR s.class NOT IN ({_excl}))
          AND s.bbox.xmin <= b.xmax AND s.bbox.xmax >= b.xmin
          AND s.bbox.ymin <= b.ymax AND s.bbox.ymax >= b.ymin
          AND ST_Intersects(s.geometry, u.geom)
    """)
    rail_count = con.execute(
        "SELECT country, count(*) AS rail FROM rail GROUP BY country ORDER BY country"
    ).fetchdf()
    rail_count
    return (rail_count,)


@app.cell
def _(con, extract_globs, rail_count, region_areas):
    # Step 4: water extract - Overture base/water whose bbox overlaps a rail segment's bbox in the
    # same country. The extract keeps any water whose envelope overlaps the country window, which
    # reaches well past it for a single large body like the North Sea; the region-bbox prefilter
    # and then the range-join to rail envelopes cut that down to water near a rail corridor.
    # Envelope columns are retained so the crossings step can range-join rail<->water cheaply. A
    # water body is kept once per country, since two countries can each meet the same river.
    _ = (rail_count, region_areas)  # dataflow: run after rail + regions
    con.execute(f"""
        CREATE OR REPLACE TABLE water AS
        WITH cand AS (
            SELECT e.country, w.id, w.subtype, w.class, w.geometry,
                   w.bbox.xmin AS min_lon, w.bbox.xmax AS max_lon,
                   w.bbox.ymin AS min_lat, w.bbox.ymax AS max_lat
            FROM read_parquet({extract_globs("base", "water")}) w
            JOIN extracts e ON e.extract_id = w.extract_id
            JOIN region_bbox b ON b.country = e.country
            WHERE w.bbox.xmin <= b.xmax AND w.bbox.xmax >= b.xmin
              AND w.bbox.ymin <= b.ymax AND w.bbox.ymax >= b.ymin
        )
        SELECT DISTINCT ON (c.country, c.id)
               c.country, c.id, c.subtype, c.class, c.geometry,
               c.min_lon, c.max_lon, c.min_lat, c.max_lat
        FROM cand c JOIN rail r
          ON r.country = c.country
         AND r.min_lon <= c.max_lon AND r.max_lon >= c.min_lon
         AND r.min_lat <= c.max_lat AND r.max_lat >= c.min_lat
    """)
    water_count = con.execute(
        "SELECT country, count(*) AS water FROM water GROUP BY country ORDER BY country"
    ).fetchdf()
    water_count
    return (water_count,)


@app.cell
def _(con, water_count):
    _ = (water_count,)  # dataflow: run after water
    con.execute("""
        CREATE OR REPLACE TABLE crossings AS
        SELECT r.country, r.id AS rail_id, r.class AS rail_class,
               w.id AS water_id, w.subtype AS water_subtype, w.class AS water_class,
               ST_Intersection(r.geometry, w.geometry) AS geom
        FROM rail r JOIN water w
          ON r.country = w.country
         AND r.min_lon <= w.max_lon AND r.max_lon >= w.min_lon
         AND r.min_lat <= w.max_lat AND r.max_lat >= w.min_lat
         AND ST_Intersects(r.geometry, w.geometry)
    """)
    crossings_count = con.execute(
        "SELECT country, count(*) AS crossings FROM crossings"
        " GROUP BY country ORDER BY country"
    ).fetchdf()
    crossings_count
    return (crossings_count,)


@app.cell
def _(
    EXCLUDE_RAIL_FLAGS,
    EXTRACTS,
    MIN_CROSSING_M,
    SUBSTANTIAL_WATER_CLASSES,
    con,
    crossings_count,
    lookout_medallion,
):
    _ = (crossings_count,)  # dataflow: run after crossings
    _subst = ", ".join(f"'{c}'" for c in SUBSTANTIAL_WATER_CLASSES)
    _excl_flags = ", ".join(f"'{f}'" for f in EXCLUDE_RAIL_FLAGS)
    # An overlap is a length in metres, so it is measured in the country's own zone: one branch
    # per country, each naming the zone the store projects that country into. A single zone would
    # measure one of them off a distant central meridian.
    _sized = " UNION ALL ".join(
        f"""
            SELECT *,
                   CAST(ST_GeometryType(part) AS VARCHAR) AS part_type,
                   ST_Length(ST_Transform(part, 'EPSG:4326',
                       '{lookout_medallion.projected_crs(country)}')) AS overlap_m,
                   ST_Centroid(part) AS cpt
            FROM parts
            WHERE NOT ST_IsEmpty(part) AND country = '{country}'
        """
        for country in EXTRACTS
    )
    con.execute(f"""
        CREATE OR REPLACE TABLE crossing_points AS
        WITH parts AS (
            SELECT country, rail_id, rail_class, water_id, water_subtype, water_class,
                   (UNNEST(ST_Dump(geom))).geom AS part
            FROM crossings
            WHERE NOT ST_IsEmpty(geom)
        ),
        sized AS ({_sized}),
        kept AS (
            SELECT row_number() OVER () AS rid, *
            FROM sized
            WHERE (part_type LIKE '%LINESTRING%' AND overlap_m > {MIN_CROSSING_M})
               OR (part_type LIKE '%POINT%' AND water_class IN ({_subst}))
        ),
        located AS (  -- V7: %-distance of the crossing along its rail segment + that segment's flags
            SELECT k.*, ST_LineLocatePoint(r.geometry, k.cpt) AS frac, r.rail_flags AS rail_flags
            FROM kept k JOIN rail r ON r.id = k.rail_id AND r.country = k.country
        ),
        redundant AS (  -- V4: point crossings whose location lies inside an areal water polygon
            SELECT DISTINCT l.rid
            FROM located l
            JOIN water wp
              ON wp.country = l.country
             AND l.part_type LIKE '%POINT%'
             AND CAST(ST_GeometryType(wp.geometry) AS VARCHAR) IN ('POLYGON', 'MULTIPOLYGON')
             AND wp.min_lon <= ST_X(l.cpt) AND wp.max_lon >= ST_X(l.cpt)
             AND wp.min_lat <= ST_Y(l.cpt) AND wp.max_lat >= ST_Y(l.cpt)
             AND ST_Contains(wp.geometry, l.cpt)
        ),
        blocked AS (  -- V7: crossing lies in a view-blocking (tunnel/covered) stretch of the segment
            SELECT DISTINCT l.rid
            FROM located l, UNNEST(l.rail_flags) AS t(f), UNNEST(f.values) AS v(flag)
            WHERE flag IN ({_excl_flags})
              AND (f.between IS NULL OR l.frac BETWEEN f.between[1] AND f.between[2])
        )
        SELECT country, rail_id, rail_class, water_id, water_subtype, water_class,
               overlap_m,
               CASE WHEN part_type LIKE '%LINESTRING%' THEN 'line' ELSE 'point' END AS overlap_kind,
               frac,
               cpt AS geom,
               ST_X(cpt) AS lon,
               ST_Y(cpt) AS lat
        FROM located
        WHERE rid NOT IN (SELECT rid FROM redundant)
          AND rid NOT IN (SELECT rid FROM blocked)
    """)
    crossing_points_count = con.execute(
        "SELECT country, count(*) AS crossing_points FROM crossing_points"
        " GROUP BY country ORDER BY country"
    ).fetchdf()
    crossing_points_count
    return (crossing_points_count,)


@app.cell
def _(
    CITY_MIN_POPULATION,
    con,
    crossing_points_count,
    extract_globs,
    region_areas,
):
    # City points for orientation on the map: Overture localities within the region above a
    # population cutoff. bbox prefilter prunes the partition; region_union clips precisely.
    _ = (crossing_points_count, region_areas)  # dataflow: keep near the pipeline tail
    con.execute(f"""
        CREATE OR REPLACE TABLE cities AS
        SELECT e.country, d.names.primary AS name, d.population,
               ST_X(d.geometry) AS lon, ST_Y(d.geometry) AS lat, d.geometry AS geom
        FROM read_parquet({extract_globs("divisions", "division")}) d
        JOIN extracts e ON e.extract_id = d.extract_id
        JOIN region_bbox b ON b.country = e.country
        JOIN region_union u ON u.country = e.country
        WHERE d.country = e.country AND d.subtype = 'locality'
          AND d.population >= {CITY_MIN_POPULATION}
          AND d.bbox.xmin <= b.xmax AND d.bbox.xmax >= b.xmin
          AND d.bbox.ymin <= b.ymax AND d.bbox.ymax >= b.ymin
          AND ST_Intersects(d.geometry, u.geom)
    """)
    cities_count = con.execute(
        "SELECT country, count(*) AS cities FROM cities GROUP BY country ORDER BY country"
    ).fetchdf()
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
    _ = (crossing_points_count, cities_count)  # dataflow: after the pipeline
    rail_gdf = to_gdf("SELECT country, id, class, geometry AS geom FROM rail")
    points_gdf = to_gdf(
        "SELECT country, rail_id, rail_class, water_id, water_subtype, water_class, overlap_m, overlap_kind, frac, lon, lat, geom FROM crossing_points"
    )
    cities_gdf = to_gdf(
        "SELECT country, name, population, lon, lat, geom FROM cities"
    )
    _water_crossed = """
        SELECT country, id, subtype, geometry AS geom FROM water
        WHERE (country, id) IN (SELECT DISTINCT country, water_id FROM crossing_points)
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
    _ = crossing_points_count  # dataflow: after the pipeline
    import sys as _sys
    from pathlib import Path as _Path

    _here = _Path(__file__).parent
    if str(_here) not in _sys.path:
        _sys.path.insert(0, str(_here))
    import crossing_ids

    def track_colors(ids):
        """RGB uint8 per track — matplotlib's categorical tab20, cycled.

        A track's id says what it is, not where it sorts, so the ids are factorised to get the
        index into the palette. Which colour a track gets therefore still depends on row order;
        nothing but this map reads it.
        """
        import numpy as np
        import pandas as pd
        import matplotlib as mpl

        cmap = mpl.colormaps["tab20"]
        codes = pd.factorize(pd.Series(list(ids)))[0]
        return (
            np.asarray([cmap(int(c) % 20)[:3] for c in codes]) * 255
        ).astype("uint8")

    # A track is a connected run of rail segments, named by the smallest segment id in it —
    # see `crossing_ids`. The name follows from the members, so it is the same across runs and
    # survives a re-extraction that leaves those segments alone.
    # Segments in two countries share no connector, so the runs this finds never span a border and
    # one pass over both countries names the same tracks two passes would.
    track = crossing_ids.track_ids(
        con.execute(
            "SELECT id, connectors FROM rail WHERE id IN (SELECT DISTINCT rail_id FROM crossing_points)"
        ).fetchall()
    )

    seg_components_gdf = to_gdf(
        "SELECT id, geometry AS geom FROM rail WHERE id IN (SELECT DISTINCT rail_id FROM crossing_points)"
    )
    seg_components_gdf["track_id"] = seg_components_gdf["id"].map(track)
    _colors = track_colors(seg_components_gdf["track_id"])
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
    return crossing_ids, track, track_colors


@app.cell
def _(lonboard, lookout_medallion, points_gdf, rail_gdf, track, track_colors):
    import numpy as _np

    parts_gdf = points_gdf.reset_index(drop=True).copy()
    parts_gdf["track_id"] = parts_gdf["rail_id"].map(track)
    # Metres, each country in its own zone: the clustering below measures distances in these
    # coordinates, and a zone chosen for one country measures the other off a distant meridian.
    # Two zones put unrelated points at the same coordinates, which is why the cluster key names
    # the country.
    parts_xy = _np.zeros((len(parts_gdf), 2))
    for _country, _labels in parts_gdf.groupby("country").groups.items():
        _at = parts_gdf.index.get_indexer(_labels)
        _proj = parts_gdf.loc[_labels].to_crs(
            lookout_medallion.projected_crs(_country)
        )
        parts_xy[_at, 0] = _proj.geometry.x.to_numpy()
        parts_xy[_at, 1] = _proj.geometry.y.to_numpy()

    _colors = track_colors(parts_gdf["track_id"])
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
    crossing_ids,
    lonboard,
    merge_dist,
    parts_gdf,
    parts_xy,
    rail_gdf,
    track_colors,
):
    import numpy as _np
    import scipy.spatial as _sps

    _D = float(merge_dist.value)
    _key = (
        parts_gdf["country"].astype(str)
        + "|"
        + parts_gdf["track_id"].astype(str)
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
        _cluster=crossing_ids.component_labels(len(parts_gdf), _edges)
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
    # A crossing is named by the water, the track, and where along the track they meet — see
    # `crossing_ids`. The position is in the name because one track crosses one body of water
    # more than once: naming by the water and the track alone gives 393 of the pairs here a
    # shared name, one of them thirteen times over.
    reps_v5_gdf["crossing_id"] = crossing_ids.crossing_ids(
        reps_v5_gdf["water_id"],
        reps_v5_gdf["track_id"],
        reps_v5_gdf["rail_id"],
        reps_v5_gdf["frac"],
    )
    # The same crossing in four bytes, for a device with no room for the id above. Minted here
    # beside the id it hashes, so nothing downstream has to know how the two relate; the store
    # refuses the write if two crossings land on one of them.
    reps_v5_gdf["crossing_compact_id"] = crossing_ids.compact_ids(
        reps_v5_gdf["crossing_id"]
    )

    _colors = track_colors(reps_v5_gdf["track_id"])
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
def _(mo, reps_v5_gdf):
    _ = reps_v5_gdf  # dataflow: what the write below would write

    write_crossings = mo.ui.run_button(
        label="write water_crossing to silver"
    )
    mo.vstack(
        [
            mo.md(
                "**Write the collapsed crossings into the store.** This replaces the whole "
                "`water_crossing` dataset for this country with what the tuning above "
                "produced — so it is a button rather than something the sliders do. "
                "Run as a script and it writes without asking, which is how a rebuild of "
                "the dataset is driven."
            ),
            write_crossings,
        ]
    )
    return (write_crossings,)


@app.cell
def _(
    EXTRACTS,
    MEDALLION_ROOT,
    MIN_CROSSING_M,
    lookout_medallion,
    merge_dist,
    mo,
    reps_v5_gdf,
    write_crossings,
):
    mo.stop(
        mo.running_in_notebook() and not write_crossings.value,
        mo.md("Nothing written — press the button above."),
    )
    import pyarrow as _pa

    # The dataset's own columns, then its two geometries, then the column its partition is
    # read from. The store checks this against the dataset's definition and refuses anything
    # that is not it, so there is nothing here about where the files go.
    _reps = reps_v5_gdf
    _rows = len(_reps)
    # Each country's geometry in its own zone, in the row order the table is built from: the store
    # stamps a partition's projected column with that country's CRS, so a row projected into
    # another country's zone would declare one thing and hold another.
    _projected = _reps.geometry.to_wkb().copy()
    for _country, _at in _reps.groupby("country").groups.items():
        _projected.loc[_at] = (
            _reps.loc[_at]
            .to_crs(lookout_medallion.projected_crs(_country))
            .geometry.to_wkb()
        )
    crossings_table = _pa.table(
        {
            "crossing_id": _pa.array(_reps["crossing_id"], _pa.string()),
            "crossing_compact_id": _pa.array(
                _reps["crossing_compact_id"], _pa.uint32()
            ),
            "water_id": _pa.array(_reps["water_id"], _pa.string()),
            "water_subtype": _pa.array(_reps["water_subtype"], _pa.string()),
            "water_class": _pa.array(_reps["water_class"], _pa.string()),
            "track_id": _pa.array(_reps["track_id"], _pa.string()),
            "rail_id": _pa.array(_reps["rail_id"], _pa.string()),
            "rail_class": _pa.array(_reps["rail_class"], _pa.string()),
            "overlap_kind": _pa.array(_reps["overlap_kind"], _pa.string()),
            "overlap_m": _pa.array(_reps["overlap_m"], _pa.float64()),
            "total_overlap_m": _pa.array(
                _reps["total_overlap_m"], _pa.float64()
            ),
            "merged_parts": _pa.array(_reps["merged_parts"], _pa.uint32()),
            "frac": _pa.array(_reps["frac"], _pa.float64()),
            # Provenance and tuning: which extract these came out of, and what this run
            # collapsed them with, so a row stays interpretable after either changes.
            "extract_id": _pa.array(
                _reps["country"].map(EXTRACTS), _pa.string()
            ),
            "merge_distance_m": _pa.array(
                [float(merge_dist.value)] * _rows, _pa.float64()
            ),
            "min_crossing_m": _pa.array(
                [float(MIN_CROSSING_M)] * _rows, _pa.float64()
            ),
            "geometry": _pa.array(_reps.geometry.to_wkb(), _pa.binary()),
            "geometry_projected": _pa.array(_projected, _pa.binary()),
            "country": _pa.array(_reps["country"], _pa.string()),
        }
    )

    written_crossings = lookout_medallion.write_silver(
        "water_crossing", crossings_table, root=str(MEDALLION_ROOT)
    )
    # A script run is how the dataset gets rebuilt, so it says what it did; in the notebook
    # the value below is the cell's output already.
    if not mo.running_in_notebook():
        print(written_crossings)
    written_crossings
    return (crossings_table,)


@app.cell
def _(MEDALLION_ROOT, con, crossings_table, gpd):
    # What actually landed in the store, read back through an engine that had no part in
    # writing it. Everything below checks or draws this rather than the frame in memory: the
    # cases are a check on the dataset the predictor and the ground truth will read, so a
    # crossing the write dropped or moved has to show up here.
    _ = crossings_table  # dataflow: after the write
    _written = con.execute(f"""
        SELECT crossing_id, water_id, water_class, water_subtype, track_id, rail_id,
               overlap_kind, overlap_m, total_overlap_m, merged_parts, frac,
               -- the position as plain numbers too, for the case-capture tool, which filters
               -- by the map's visible bounds rather than by a geometry predicate
               ST_X(geometry) AS lon, ST_Y(geometry) AS lat,
               ST_AsWKB(geometry) AS wkb
        FROM read_parquet('{MEDALLION_ROOT}/silver/water_crossing/**/*.parquet')
    """).fetchdf()
    silver_crossings_gdf = gpd.GeoDataFrame(
        _written.drop(columns=["wkb"]),
        geometry=gpd.GeoSeries.from_wkb(
            _written.pop("wkb").map(bytes), crs="EPSG:4326"
        ),
    )
    len(silver_crossings_gdf)
    return (silver_crossings_gdf,)


@app.cell
def _(mo, rail_gdf, silver_crossings_gdf):
    import sys as _sys
    from pathlib import Path as _Path

    _here = _Path(__file__).parent
    if str(_here) not in _sys.path:
        _sys.path.insert(0, str(_here))
    import crossing_checks as _cc

    test_cases = _cc.load_cases(_here / "test_cases.geojson")
    test_results = _cc.run_cases(test_cases, silver_crossings_gdf, rail_gdf)
    if not mo.running_in_notebook():
        print(test_results.to_string(index=False))
    test_results
    return


@app.cell
def raw_candidates(crossing_points_count, to_gdf):
    _ = crossing_points_count  # dataflow: after the pipeline
    raw_crossings_gdf = to_gdf("""
        SELECT rail_id, water_class,
               ST_Centroid((UNNEST(ST_Dump(geom))).geom) AS geom
        FROM crossings WHERE NOT ST_IsEmpty(geom)
    """)
    len(raw_crossings_gdf)
    return (raw_crossings_gdf,)


@app.cell
def test_pick(mo):
    import importlib as _il
    import sys as _sys
    from pathlib import Path as _P

    _vd = _P(__file__).parent
    if str(_vd) not in _sys.path:
        _sys.path.insert(0, str(_vd))
    import crossing_checks as _ccv
    import test_viz

    test_viz = _il.reload(
        test_viz
    )  # pick up module edits without a kernel restart
    viz_cases = _ccv.load_cases(_vd / "test_cases.geojson")
    test_case_pick = mo.ui.dropdown(
        options=list(viz_cases["name"]),
        value=viz_cases["name"].iloc[0],
        label="test case",
    )
    test_case_pick
    return test_case_pick, test_viz, viz_cases


@app.cell
def test_view(
    rail_gdf,
    raw_crossings_gdf,
    silver_crossings_gdf,
    test_case_pick,
    test_viz,
    viz_cases,
    water_lines_gdf,
    water_polys_gdf,
):
    _case = viz_cases[viz_cases["name"] == test_case_pick.value].iloc[0]
    test_viz.case_view(
        _case,
        rail_gdf,
        silver_crossings_gdf,
        water_polys_gdf=water_polys_gdf,
        water_lines_gdf=water_lines_gdf,
        raw_gdf=raw_crossings_gdf,
    )
    return


@app.cell
def _(cities_gdf, rail_gdf, silver_crossings_gdf):
    import importlib as _il
    import sys as _sys
    from pathlib import Path as _Path

    _nbdir = _Path(__file__).parent
    if str(_nbdir) not in _sys.path:
        _sys.path.insert(0, str(_nbdir))
    import bbox_capture

    bbox_capture = _il.reload(
        bbox_capture
    )  # pick up module edits without a kernel restart

    cases_path = _nbdir / "test_cases.geojson"
    capture_map, case_name, case_expected, refresh_button, append_button = (
        bbox_capture.make_capture(silver_crossings_gdf, rail_gdf, cities_gdf)
    )
    bbox_capture.controls(
        capture_map, case_name, case_expected, refresh_button, append_button
    )
    return (
        append_button,
        bbox_capture,
        capture_map,
        case_expected,
        case_name,
        cases_path,
        refresh_button,
    )


@app.cell
def _(
    append_button,
    bbox_capture,
    capture_map,
    case_expected,
    case_name,
    cases_path,
    refresh_button,
    silver_crossings_gdf,
):
    bbox_capture.result(
        capture_map,
        case_name,
        case_expected,
        refresh_button,
        append_button,
        silver_crossings_gdf,
        cases_path,
    )
    return


if __name__ == "__main__":
    app.run()
