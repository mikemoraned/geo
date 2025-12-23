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
    # Goals

    * Find all London subway stations
    * Explore:
        * OvertureMaps `segments`/`connector` transport dataset, as well as `base` dataset
        * Using GeoPandas / Marimo / Duckdb Spatial

    ## Approach

    Assume that connectors between subways segments are tube station.

    ## Conclusion

    Using "segments"/"connector" datasets I think what I've actually found is not subway stations, but instead subway platforms, including some that appear to be under the water 😀.

    Whilst it was fun to play with the transport data, it is far simpler to use the `base` `infrastructure` dataset with a filter of `subtype == transit` and `class == subway_station`.
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
        conn.install_extension('spatial', force_install=False)
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
def _():
    from shapely.geometry import box

    def overlapping_bbox(gdf, bbox):
        (minx,miny,maxx,maxy) = bbox
    
        overlapping_gdf = gdf[gdf.intersects(box(minx, miny, maxx, maxy))]

        return overlapping_gdf

    def centroid_as_latlon(bbox):
        (minx,miny,maxx,maxy) = bbox
        centroid = box(minx, miny, maxx, maxy).centroid
        return [centroid.y, centroid.x]
    return centroid_as_latlon, overlapping_bbox


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
    overturemaps_base = f"/Volumes/PRO-G40/OvertureMaps/data/release/{overturemaps_release}" # this is a local copy I have on a drive
    return (overturemaps_base,)


@app.cell
def _():
    # London bbox based on example on https://wiki.openstreetmap.org/wiki/Bounding_box
    london_bbox = (-0.489,51.28,0.236,51.686)
    return (london_bbox,)


@app.cell
def _():
    # interactively selected in https://boundingbox.klokantech.com
    kingcross_area_bbox = (-0.138,51.527,-0.118,51.533)
    return (kingcross_area_bbox,)


@app.cell
def _(conn, load_area, london_bbox, overturemaps_base):
    london_segments_gdf = load_area(conn, overturemaps_base, "transportation", "segment", london_bbox)
    return (london_segments_gdf,)


@app.cell
def _(
    centroid_as_latlon,
    kingcross_area_bbox,
    london_segments_gdf,
    overlapping_bbox,
):
    # transport segments overlapping kings cross area
    overlapping_bbox(london_segments_gdf, kingcross_area_bbox).explore(
        tiles="CartoDB positron", location=centroid_as_latlon(kingcross_area_bbox), zoom_start=15)
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
    return (london_subway_segment_gdf,)


@app.cell
def _(
    centroid_as_latlon,
    kingcross_area_bbox,
    london_subway_segment_gdf,
    overlapping_bbox,
):
    # segments overlapping kings cross area
    overlapping_bbox(london_subway_segment_gdf, kingcross_area_bbox).explore(
        tiles="CartoDB positron", location=centroid_as_latlon(kingcross_area_bbox), zoom_start=15)
    return


@app.cell
def _(conn, load_area, london_bbox, overturemaps_base):
    london_connectors_gdf = load_area(conn, overturemaps_base, "transportation", "connector", london_bbox)
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
    return (london_connectors_restricted_gdf,)


@app.cell
def _(london_connectors_restricted_gdf):
    london_connectors_restricted_gdf.explore(tiles="CartoDB positron", color="red")
    return


@app.cell
def _(
    centroid_as_latlon,
    kingcross_area_bbox,
    london_connectors_restricted_gdf,
    london_subway_segment_gdf,
    overlapping_bbox,
):
    m = overlapping_bbox(london_subway_segment_gdf, kingcross_area_bbox).explore(
        tiles="CartoDB positron", location=centroid_as_latlon(kingcross_area_bbox), zoom_start=16, color="blue")
    overlapping_bbox(london_connectors_restricted_gdf, kingcross_area_bbox).explore(m=m, color="red")
    return


@app.cell
def _(
    london_connectors_restricted_gdf,
    london_subway_segment_gdf,
    overlapping_bbox,
):
    embankment_bbox = (-0.1252405125,51.5040095967,-0.1151554066,51.5083268675)
    embankment = overlapping_bbox(london_subway_segment_gdf, embankment_bbox).explore(
        tiles="CartoDB positron", location=[51.5061, -0.1216], zoom_start=17, color="blue")
    overlapping_bbox(london_connectors_restricted_gdf, embankment_bbox).explore(m=embankment, color="red")
    return (embankment_bbox,)


@app.cell
def _(conn, load_area, london_bbox, overturemaps_base):
    # try using base, guided by https://docs.overturemaps.org/schema/concepts/by-theme/base/
    london_infra_gdf = load_area(conn, overturemaps_base, "base", "infrastructure", london_bbox)
    return (london_infra_gdf,)


@app.cell
def _(london_infra_gdf):
    london_infra_gdf.head()
    return


@app.cell
def _(
    centroid_as_latlon,
    kingcross_area_bbox,
    london_infra_gdf,
    overlapping_bbox,
):
    # infra overlapping kings cross area
    overlapping_bbox(london_infra_gdf, kingcross_area_bbox).explore(
        tiles="CartoDB positron", location=centroid_as_latlon(kingcross_area_bbox), zoom_start=16, color="green")
    return


@app.cell
def _(london_infra_gdf):
    london_infra_subway_gdf = london_infra_gdf[
        (london_infra_gdf['subtype'] == 'transit') 
        & (london_infra_gdf['class'] == 'subway_station')]
    return (london_infra_subway_gdf,)


@app.cell
def _(
    centroid_as_latlon,
    kingcross_area_bbox,
    london_infra_subway_gdf,
    overlapping_bbox,
):
    # subways overlapping kings cross area
    overlapping_bbox(london_infra_subway_gdf, kingcross_area_bbox).explore(
        tiles="CartoDB positron", location=centroid_as_latlon(kingcross_area_bbox), zoom_start=16, color="green")
    return


@app.cell
def _(
    centroid_as_latlon,
    embankment_bbox,
    london_infra_subway_gdf,
    overlapping_bbox,
):
    # subways overlapping embankment area
    overlapping_bbox(london_infra_subway_gdf, embankment_bbox).explore(
        tiles="CartoDB positron", location=centroid_as_latlon(embankment_bbox), zoom_start=16, color="green")
    return


@app.cell
def _(london_infra_subway_gdf):
    # subways of london
    london_infra_subway_gdf.explore(tiles="CartoDB positron", color="green")
    return


if __name__ == "__main__":
    app.run()
