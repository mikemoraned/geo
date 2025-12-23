import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    return


@app.cell
def _():
    import duckdb
    import geopandas as gpd
    import pandas as pd
    from shapely import wkt
    return duckdb, gpd, wkt


@app.cell
def _(duckdb):
    def duckdb_connection():
        conn = duckdb.connect()
        conn.install_extension('spatial')
        conn.load_extension('spatial')
        return conn
    return (duckdb_connection,)


@app.cell
def _(gpd, wkt):
    def load_area(conn, overture_local_base, overture_theme, overture_type, bbox):
        (minx,miny,maxx,maxy) = bbox
        path = f"{overture_local_base}/theme={overture_theme}/type={overture_type}/*"
        query = f"""
        SELECT * EXCLUDE(geometry), ST_AsText(geometry) AS geometry 
        FROM read_parquet('{path}')
        WHERE bbox.xmin >= {minx}
          AND bbox.xmax <= {maxx}
          AND bbox.ymin >= {miny}
          AND bbox.ymax <= {maxy}
        """
        arrow_table = conn.execute(query).fetch_arrow_table()
        df = arrow_table.to_pandas()
        gdf = gpd.GeoDataFrame(
            df,
            geometry=df['geometry'].apply(wkt.loads),
            crs="EPSG:4326"
        )
        return gdf
    return (load_area,)


@app.cell
def _(duckdb_connection):
    conn = duckdb_connection()
    return (conn,)


@app.cell
def _():
    overturemaps_release = "2025-12-17.0"
    overturemaps_base = f"/Volumes/PRO-G40/OvertureMaps/data/release/{overturemaps_release}"
    return (overturemaps_base,)


@app.cell
def _():
    # London bbox based on example on https://wiki.openstreetmap.org/wiki/Bounding_box
    # We could derive this based on the GERS id, but not for now
    london_bbox = (-0.489,51.28,0.236,51.686)
    return (london_bbox,)


@app.cell
def _(conn, load_area, london_bbox, overturemaps_base):
    london_segments_gdf = load_area(conn, overturemaps_base, "transportation", "segment", london_bbox)
    return (london_segments_gdf,)


@app.cell
def _(london_segments_gdf):
    london_segments_gdf.head()
    return


@app.cell
def _(london_segments_gdf):
    from lonboard import Map, PathLayer

    london_layer = PathLayer.from_geopandas(
            london_segments_gdf,
            get_color=[0, 128, 255, 180],
            get_width=100
    )
    return Map, london_layer


@app.cell
def _(Map, london_layer):
    m = Map([london_layer])
    m
    return


if __name__ == "__main__":
    app.run()
