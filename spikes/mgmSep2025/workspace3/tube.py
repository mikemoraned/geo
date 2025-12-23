import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    return (mo,)


@app.cell
def _(mo):
    mo.md(r"""
    # Goal

    Show all London subway stations
    """)
    return


@app.cell
def _():
    # Imports and generic utility methods
    return


@app.cell
def _():
    import duckdb
    import geopandas as gpd
    import pandas as pd
    from shapely import wkt
    return duckdb, gpd, pd, wkt


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
    def load_by_ids(conn, overture_local_base, gers_ids):
        path = f"{overture_local_base}/theme=divisions/type=division_area/*"
        gers_ids_joined = ",".join([f"'{g}'" for g in gers_ids])
        query = f"""
        SELECT * EXCLUDE(geometry), ST_AsText(geometry) AS geometry 
        FROM read_parquet('{path}')
        WHERE id IN ({gers_ids_joined})
        """
        arrow_table = conn.execute(query).fetch_arrow_table()
        df = arrow_table.to_pandas()
        gdf = gpd.GeoDataFrame(
            df,
            geometry=df['geometry'].apply(wkt.loads),
            crs="EPSG:4326"
        )
        return gdf
    return (load_by_ids,)


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


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Load London area
    """)
    return


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
    gers_ids = [
        "531326d0-51f4-4c9e-8af5-d704aeea7830", # City of Westminster
        "89c092f8-4287-4401-b72f-4a5a067eee22", # City of London
        "5e0a58c5-df70-47c4-a71d-52235d3cb6d5"  # London Borough of Tower Hamlets
    ] 
    return (gers_ids,)


@app.cell
def _(conn, gers_ids, load_by_ids, overturemaps_base):
    london_regions_gdf = load_by_ids(conn, overturemaps_base, gers_ids)
    return (london_regions_gdf,)


@app.cell
def _(london_regions_gdf):
    london_regions_gdf.explore()
    return


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
def _(mo):
    mo.md(r"""
    ## Find subway connectors

    Find the connectors that are attached to segments that are labelled as subways, implying these connectors are in the subway (tube).
    """)
    return


@app.cell
def _(pd):
    def find_subway_segment_with_connectors(gdf):
        # restrict to only those segments that belong to a subway
        subway_segment_gdf = gdf[gdf["class"] == 'subway']

        # each segment has a list of connectors (stops along it)
        # create a dataframe which contains one row for each
        # connector, along with all segment information
        exploded_gdf = subway_segment_gdf.explode("connectors")

        # create a new dataframe which contains a new
        # `connector_id` column, but is indexed by the original
        # `exploded_gdf` dataframe
        connectors_df = pd.DataFrame(exploded_gdf["connectors"].tolist(), index=exploded_gdf.index)

        # join it back to exploded df so we end up with a new 
        # `connector_id` on each repeated segment
        joined_gdf = exploded_gdf.join(connectors_df)

        return joined_gdf
    return (find_subway_segment_with_connectors,)


@app.cell
def _(find_subway_segment_with_connectors, london_segments_gdf):
    london_subway_segment_gdf = find_subway_segment_with_connectors(london_segments_gdf)
    london_subway_segment_gdf.head()
    return (london_subway_segment_gdf,)


@app.cell
def _(conn, load_area, london_bbox, overturemaps_base):
    london_connectors_gdf = load_area(conn, overturemaps_base, "transportation", "connector", london_bbox)
    london_connectors_gdf.head()
    return (london_connectors_gdf,)


@app.function
def restrict_to_connectors(connector_gdf, subset_gdf):
    subsetted_gdf = (
        connector_gdf.merge(subset_gdf.loc[:,"connector_id"],
        left_on="id",
        right_on="connector_id",
        how="inner"
    ))
    return subsetted_gdf.drop_duplicates(subset='id')


@app.cell
def _(london_connectors_gdf, london_subway_segment_gdf):
    london_connectors_restricted_gdf = restrict_to_connectors(london_connectors_gdf, london_subway_segment_gdf)
    london_connectors_restricted_gdf.head()
    return (london_connectors_restricted_gdf,)


@app.cell
def _(london_connectors_restricted_gdf, london_regions_gdf):
    m = london_connectors_restricted_gdf.explore(tiles="CartoDB positron")
    london_regions_gdf.explore(m=m)
    return


if __name__ == "__main__":
    app.run()
