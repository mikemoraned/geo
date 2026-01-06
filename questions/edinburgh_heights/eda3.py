import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    return


@app.cell
def _():
    import geopandas as gpd
    import pandas as pd
    import numpy as np
    import duckdb
    from shapely import wkt, LineString
    import matplotlib.pyplot as plt
    import h3
    from shapely.geometry import Polygon, shape, mapping, box
    import rasterio
    from rasterio.mask import mask
    from typing import List, Tuple
    from osgb import format_grid
    import pyarrow as pa
    from pyarrow import feather
    return (
        List,
        Polygon,
        box,
        duckdb,
        format_grid,
        gpd,
        h3,
        mapping,
        mask,
        np,
        pd,
        rasterio,
        wkt,
    )


@app.cell
def _(duckdb):
    def duckdb_connection():
        conn = duckdb.connect()
        conn.install_extension("spatial", force_install=False)
        conn.load_extension("spatial")
        return conn
    return (duckdb_connection,)


@app.cell
def _(gpd, wkt):
    def query_to_dataframe(conn, query):
        arrow_table = conn.execute(query).fetch_arrow_table()
        df = arrow_table.to_pandas()
        gdf = gpd.GeoDataFrame(
            df, geometry=df["geometry"].apply(wkt.loads), crs="EPSG:4326"
        )
        return gdf
    return (query_to_dataframe,)


@app.cell
def _(query_to_dataframe):
    def load_city_boundary(conn, gers_id, release="2025-12-17.0"):
        query = f"""
            SELECT * EXCLUDE(geometry), ST_AsText(geometry) AS geometry 
            FROM read_parquet('s3://overturemaps-us-west-2/release/{release}/theme=divisions/type=division_area/*', filename=true, hive_partitioning=1)
            WHERE id = '{gers_id}'
            """
        return query_to_dataframe(conn, query)
    return (load_city_boundary,)


@app.cell
def _(duckdb_connection):
    conn = duckdb_connection()
    return (conn,)


@app.cell
def _(conn, load_city_boundary):
    edinburgh_gers = "58a34fa4-bc76-476e-81a8-1ed8a5cd693f"
    edinburgh_gdf = load_city_boundary(conn, edinburgh_gers)
    return (edinburgh_gdf,)


@app.cell
def _(edinburgh_gdf):
    edinburgh_gdf.explore()
    return


@app.cell
def _(edinburgh_gdf, gpd):
    # the boundary looks a bit over simplified and is missing-out bits of the coast. so, bloat a bit to recover the coast
    def buffer(gdf: gpd.GeoDataFrame, buffer_metres: float) -> gpd.GeoDataFrame:
        original_crs = gdf.crs
        gdf_buffered = gdf.to_crs(epsg=27700).copy() # EPSG 27700 has a unit of metres so is valid to use here
        gdf_buffered["geometry"] = gdf_buffered.geometry.buffer(buffer_metres)
        return gdf_buffered.to_crs(original_crs)

    edinburgh_buffered_gdf = buffer(edinburgh_gdf, buffer_metres=400)
    return (edinburgh_buffered_gdf,)


@app.cell
def _(edinburgh_buffered_gdf):
    edinburgh_buffered_gdf.explore()
    return


@app.cell
def _(Polygon, gpd, h3):
    def get_h3_cells_for_gdf(gdf, resolution):
        """
        Get all H3 cells at a given resolution that cover the geometry in a GeoDataFrame.

        Parameters:
        - gdf: GeoDataFrame with a single row
        - resolution: H3 resolution (0-15)

        Returns:
        - GeoDataFrame with H3 cell polygons
        """
        # Ensure we're working in WGS84 (required by H3)
        gdf_wgs84 = gdf.to_crs(epsg=4326)

        # Get the geometry
        geom = gdf_wgs84.geometry.iloc[0]

        # Get H3 cells covering the polygon
        h3_cells = h3.geo_to_cells(geom, res=resolution)

        # Convert H3 cells to polygons
        polygons = []
        for cell in h3_cells:
            boundary = h3.cell_to_boundary(cell)
            # h3 returns (lat, lng) tuples, need to flip to (lng, lat) for Shapely
            polygon = Polygon([(lng, lat) for lat, lng in boundary])
            polygons.append(polygon)

        # Create new GeoDataFrame
        result_gdf = gpd.GeoDataFrame(
            {"h3_index": list(h3_cells)}, geometry=polygons, crs="EPSG:4326"
        )

        return result_gdf


    # Usage:
    # h3_gdf = get_h3_cells_for_gdf(my_gdf, resolution=9)
    return (get_h3_cells_for_gdf,)


@app.cell
def _(edinburgh_buffered_gdf, get_h3_cells_for_gdf):
    h3_resolution = 10
    edinburgh_h3_cells_gdf = get_h3_cells_for_gdf(edinburgh_buffered_gdf, h3_resolution)
    edinburgh_h3_cells_gdf.head()
    return edinburgh_h3_cells_gdf, h3_resolution


@app.cell
def _(edinburgh_h3_cells_gdf):
    edinburgh_h3_cells_gdf.explore()
    return


@app.cell
def _(box, format_grid, gpd):
    def get_bng_quadrants(gdf):
        """Find all BNG quadrant cells (e.g. NT16NE) intersecting a GeoDataFrame."""
        gdf = gdf.to_crs(epsg=27700)
        geom = gdf.union_all()

        minx, miny, maxx, maxy = geom.bounds
        minx = (minx // 5000) * 5000
        miny = (miny // 5000) * 5000

        cells = []
        x = minx
        while x <= maxx:
            y = miny
            while y <= maxy:
                cell_geom = box(x, y, x + 5000, y + 5000)
                if cell_geom.intersects(geom):
                    km_ref = format_grid(x, y, form="SS E N").replace(
                        " ", ""
                    )  # NT16
                    qy = "N" if (y % 10000) >= 5000 else "S"
                    qx = "E" if (x % 10000) >= 5000 else "W"
                    cells.append(
                        {"grid_ref": km_ref + qy + qx, "geometry": cell_geom}
                    )
                y += 5000
            x += 5000

        return gpd.GeoDataFrame(cells, crs="EPSG:27700")
    return (get_bng_quadrants,)


@app.cell
def _(edinburgh_gdf, get_bng_quadrants):
    edinburgh_bng_quadrants_gdf = get_bng_quadrants(edinburgh_gdf)
    edinburgh_bng_quadrants_gdf.head()
    return (edinburgh_bng_quadrants_gdf,)


@app.cell
def _(edinburgh_bng_quadrants_gdf):
    edinburgh_bng_quadrants_gdf.explore()
    return


@app.cell
def _(edinburgh_bng_quadrants_gdf):
    edinburgh_geotiff_urls = [
        f"https://srsp-open-data.s3-eu-west-2.amazonaws.com/lidar/phase-5/dtm/27700/gridded/{ref}_50CM_DTM_PHASE5.tif"
        for ref in edinburgh_bng_quadrants_gdf["grid_ref"]
    ]
    edinburgh_geotiff_urls
    return (edinburgh_geotiff_urls,)


@app.cell
def _(box, gpd, mapping, mask, np, pd, rasterio):
    def process_geotiff(
        tiff_url: str, h3_gdf: gpd.GeoDataFrame
    ) -> gpd.GeoDataFrame:
        """
        Process a single GeoTIFF and compute minimum height
        for each H3 cell that intersects with the raster.

        Args:
            tiff_url: URL or path to the GeoTIFF file
            h3_gdf: GeoDataFrame with H3 cell polygons, indexed by h3_index (EPSG:4326)

        Returns:
            GeoDataFrame with h3_index index, geometry, and columns: min_height
            CRS is EPSG:4326 (H3 native CRS)
        """
        print(f"Opening raster: {tiff_url}")

        with rasterio.open(tiff_url) as src:
            raster_crs = src.crs
            raster_bounds = src.bounds
            nodata_value = src.nodata

            print(f"  CRS: {raster_crs}")
            print(f"  Bounds: {raster_bounds}")
            print(f"  Nodata value: {nodata_value}")

            # Reproject H3 cells to raster CRS
            print(f"  Reprojecting H3 cells from {h3_gdf.crs} to {raster_crs}")
            h3_reprojected = h3_gdf.to_crs(raster_crs)

            # Filter to H3 cells that intersect the raster bounds
            raster_bbox = box(
                raster_bounds.left,
                raster_bounds.bottom,
                raster_bounds.right,
                raster_bounds.top,
            )
            intersecting_mask = h3_reprojected.intersects(raster_bbox)
            h3_subset = h3_reprojected[intersecting_mask]

            print(f"  Found {len(h3_subset)} H3 cells intersecting raster")

            if len(h3_subset) == 0:
                return gpd.GeoDataFrame(
                    columns=["geometry", "total_height", "total_samples"],
                    crs="EPSG:4326",
                )

            # Process each H3 cell
            results = []
            for h3_index, row in h3_subset.iterrows():
                geom = [mapping(row.geometry)]
                out_image, out_transform = mask(
                    src, geom, crop=True, all_touched=True
                )
                data = out_image[0]

                # Mask out nodata values
                if nodata_value is not None:
                    valid_mask = data != nodata_value
                else:
                    valid_mask = ~np.isnan(data)

                valid_data = data[valid_mask]

                if len(valid_data) > 0:
                    results.append(
                        {
                            "h3_index": h3_index,
                            "min_height": float(np.min(valid_data))
                        }
                    )

            print(f"  Processed {len(results)} H3 cells with valid data")

        if not results:
            return gpd.GeoDataFrame(
                columns=["geometry", "min_height"],
                crs="EPSG:4326",
            )

        # Create DataFrame and join back to original h3_gdf geometry
        df = pd.DataFrame(results).set_index("h3_index")
        gdf = h3_gdf.join(df, how="inner")

        return gdf
    return (process_geotiff,)


@app.cell
def _(edinburgh_geotiff_urls, edinburgh_h3_cells_gdf, process_geotiff):
    edinburgh_height_gdfs = []
    for i, url in enumerate(edinburgh_geotiff_urls):
        print(f"Processing {i + 1}/{len(edinburgh_geotiff_urls)}: {url[:80]}...")
        gdf = process_geotiff(url, edinburgh_h3_cells_gdf)
        if len(gdf) > 0:
            edinburgh_height_gdfs.append(gdf)
            print(f"Found data for {len(gdf)} H3 cells")
        else:
            print(f"No intersecting H3 cells")
    return (edinburgh_height_gdfs,)


@app.cell
def _(edinburgh_height_gdfs):
    edinburgh_height_gdfs[20].explore("min_height")
    return


@app.cell
def _(List, gpd, pd):
    def merge_and_compute_min(
        gdfs: List[gpd.GeoDataFrame],
    ) -> gpd.GeoDataFrame:
        """
        Merge multiple height GeoDataFrames by finding min of min_height
        for each H3 cell.

        Args:
            gdfs: List of GeoDataFrames with h3_index index, geometry,
                  and columns: min_height

        Returns:
            GeoDataFrame with h3_index index, geometry, and columns:
            min_height (CRS: EPSG:4326)
        """
        # Filter out empty GeoDataFrames
        non_empty_gdfs = [gdf for gdf in gdfs if len(gdf) > 0]

        print(f"Merging {len(non_empty_gdfs)} non-empty GeoDataFrames")

        if not non_empty_gdfs:
            return gpd.GeoDataFrame(
                columns=[
                    "geometry",
                    "min_height",
                ],
                crs="EPSG:4326",
            )

        # Extract geometry from first GeoDataFrame for each h3_index
        # (all GeoDataFrames should have the same geometry for the same h3_index)
        all_geometries = pd.concat([gdf[["geometry"]] for gdf in non_empty_gdfs])
        geometries = all_geometries.groupby(level=0).first()

        # Concatenate and min the numeric columns
        all_stats = pd.concat(
            [gdf[["min_height"]] for gdf in non_empty_gdfs]
        )
        merged_stats = all_stats.groupby(level=0).agg(
            {"min_height": "min"}
        )

        # Combine geometry and stats into GeoDataFrame
        result = gpd.GeoDataFrame(merged_stats.join(geometries), crs="EPSG:4326")

        print(f"  Merged into {len(result)} H3 cells")
        print(
            f"  Height range: {result['min_height'].min():.2f} to {result['min_height'].max():.2f}"
        )

        return result
    return (merge_and_compute_min,)


@app.cell
def _(edinburgh_height_gdfs, merge_and_compute_min):
    edinburgh_height_gdf = merge_and_compute_min(edinburgh_height_gdfs)
    edinburgh_height_gdf.head()
    return (edinburgh_height_gdf,)


@app.cell
def _():
    # edinburgh_height_gdf.explore("avg_height")
    return


@app.cell
def _(edinburgh_gdf):
    edinburgh_gdf.to_feather(
        f"edinburgh.arrow", compression="uncompressed"
    )
    return


@app.cell
def _(edinburgh_buffered_gdf):
    edinburgh_buffered_gdf.to_feather(
        f"edinburgh_buffered.arrow", compression="uncompressed"
    )
    return


@app.cell
def _(edinburgh_height_gdf, h3_resolution):
    edinburgh_height_gdf.to_parquet(f"edinburgh_min_{h3_resolution}.geoparquet")
    return


@app.cell
def _(edinburgh_height_gdf, h3_resolution):
    edinburgh_height_gdf.to_feather(
        f"edinburgh_min_{h3_resolution}.arrow", compression="uncompressed"
    )
    return


if __name__ == "__main__":
    app.run()
