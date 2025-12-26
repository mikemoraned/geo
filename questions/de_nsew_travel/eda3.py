import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import geopandas as gpt
    import pandas as pd
    import overturemaps
    import duckdb
    return gpt, pd


@app.cell
def _(pd):
    travel_times = pd.read_parquet('city_pairs_diff_quadrant_travel_times.parquet')
    travel_times.head()
    return (travel_times,)


@app.cell
def _(travel_times):
    travel_times[(travel_times['success'] == True) & (travel_times['total_time'] != 0)]
    return


@app.cell
def _(gpt, travel_times):
    from shapely.geometry import LineString

    # Create line geometries from origin and destination coordinates
    travel_times_lines = travel_times[(travel_times['success'] == True)].copy()
    travel_times_lines['geometry'] = travel_times_lines.apply(
        lambda row: LineString([
            (row['lon_origin'], row['lat_origin']),
            (row['lon_dest'], row['lat_dest'])
        ]),
        axis=1
    )

    # Convert to GeoDataFrame
    travel_times_gdf = gpt.GeoDataFrame(
        travel_times_lines,
        geometry='geometry',
        crs='EPSG:4326'
    )

    # Visualize with travel time as color
    travel_times_gdf.explore(
        column='total_time',
        cmap='RdYlGn_r',
        tiles="CartoDB positron",
        style_kwds={'weight': 2, 'opacity': 0.7},
        tooltip=['id_origin', 'id_dest', 'total_time']
    )
    return (LineString,)


@app.cell
def _(LineString, gpt, travel_times):
    import polyline
    from shapely.geometry import MultiLineString

    # Decode polylines and create MultiLineString geometries
    def decode_polylines_to_multilinestring(polylines_array):
        if polylines_array is None or len(polylines_array) == 0:
            return None
        lines = []
        for encoded_polyline in polylines_array:
            if encoded_polyline:
                # polyline.decode returns list of (lat, lon) tuples, need to swap to (lon, lat) for shapely
                coords = [(lon, lat) for lat, lon in polyline.decode(encoded_polyline, precision=6)]
                if len(coords) >= 2:
                    lines.append(LineString(coords))
        if len(lines) == 0:
            return None
        return MultiLineString(lines)

    # Create new geodataframe with decoded route geometries
    travel_times_routes = travel_times[(travel_times['success'] == True)].copy()
    travel_times_routes['route_geometry'] = travel_times_routes['polylines'].apply(decode_polylines_to_multilinestring)

    travel_routes_gdf = gpt.GeoDataFrame(
        travel_times_routes.drop(columns=['polylines']),
        geometry='route_geometry',
        crs='EPSG:4326'
    )
    return (travel_routes_gdf,)


@app.cell
def _(travel_routes_gdf):
    import lonboard
    from lonboard import Map

    # Create a PathLayer for the route geometries
    path_layer = lonboard.PathLayer.from_geopandas(
        travel_routes_gdf,
        width_min_pixels=2,
    )

    Map(layers=[path_layer], basemap_style=lonboard.basemap.CartoBasemap.Positron)

    return


if __name__ == "__main__":
    app.run()
