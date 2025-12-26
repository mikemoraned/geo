import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    return


@app.cell
def _():
    import geopandas as gpt
    import pandas as pd
    import overturemaps
    import duckdb
    return gpt, overturemaps, pd


@app.cell
def _(overturemaps):
    div = overturemaps.core.geodataframe(overture_type='division')
    div = div.set_crs('EPSG:4326')
    div.head()
    return (div,)


@app.cell
def _(div):
    div.crs
    return


@app.cell
def _(div):
    de_div = div[div['country'] == 'DE']
    de_div.head()
    return (de_div,)


@app.cell
def _(de_div):
    de_div[de_div['population'] > 0].head()
    return


@app.cell
def _(de_div):
    cities_de = de_div[de_div['class'] == 'city'].sort_values('population', ascending=False)
    cities_de.head(20)
    return (cities_de,)


@app.cell
def _(cities_de):
    cities_de[['names', 'population', 'geometry']].explore("population", scheme="percentiles")
    return


@app.cell
def _():
    return


@app.cell
def _(overturemaps):
    area = overturemaps.core.geodataframe(overture_type='division_area')
    area = area.set_crs('EPSG:4326')
    area.head()
    return (area,)


@app.cell
def _(area):
    countries = area[area['subtype'] == 'country']
    return (countries,)


@app.cell
def _(countries):
    de_countries = countries[(countries['country'] == 'DE') & (countries['class'] == 'land')]
    de_countries.head()
    return (de_countries,)


@app.cell
def _(de_countries):
    de_countries.explore()
    return


@app.cell
def _(cities_de, de_countries):
    # Get the centroid of Germany
    germany_centroid = de_countries.geometry.centroid.iloc[0]

    # Create a copy of cities_de to avoid modifying the original
    cities_positioned = cities_de.copy()

    # Determine North/South position based on latitude relative to centroid
    cities_positioned['ns_position'] = cities_positioned.geometry.centroid.y.apply(
        lambda y: 'North' if y > germany_centroid.y else 'South'
    )

    # Determine East/West position based on longitude relative to centroid
    cities_positioned['ew_position'] = cities_positioned.geometry.centroid.x.apply(
        lambda x: 'East' if x > germany_centroid.x else 'West'
    )

    # Combine into a single position label
    cities_positioned['quadrant'] = cities_positioned['ns_position'] + '-' + cities_positioned['ew_position']

    cities_positioned[['names', 'population', 'ns_position', 'ew_position', 'quadrant', 'geometry']].head(20)
    return (cities_positioned,)


@app.cell
def _(cities_positioned):
    cities_positioned.explore("quadrant", categorical=True, tiles="CartoDB positron")
    return


@app.cell
def _(cities_positioned):
    # Self-join cities to create all pairs
    city_pairs = cities_positioned.merge(
        cities_positioned,
        how='cross',
        suffixes=('_origin', '_dest')
    )

    # Filter out same city pairs and pairs in the same quadrant
    city_pairs_diff_quadrant = city_pairs[
        (city_pairs['id_origin'] != city_pairs['id_dest']) &
        (city_pairs['quadrant_origin'] != city_pairs['quadrant_dest'])
    ]

    city_pairs_diff_quadrant[['names_origin', 'quadrant_origin', 'names_dest', 'quadrant_dest', 'population_origin', 'population_dest']].head(20)
    return (city_pairs_diff_quadrant,)


@app.cell
def _(city_pairs_diff_quadrant, gpt):
    from shapely.geometry import LineString

    # Create line geometries between origin and destination cities
    city_pairs_lines = city_pairs_diff_quadrant.copy()
    city_pairs_lines['line_geometry'] = city_pairs_lines.apply(
        lambda row: LineString([
            row['geometry_origin'].centroid,
            row['geometry_dest'].centroid
        ]),
        axis=1
    )

    # Convert to GeoDataFrame with line geometry
    city_pairs_gdf = gpt.GeoDataFrame(
        city_pairs_lines[['names_origin', 'names_dest', 'quadrant_origin', 'quadrant_dest', 'population_origin', 'population_dest']],
        geometry=city_pairs_lines['line_geometry'],
        crs='EPSG:4326'
    )

    city_pairs_gdf.sample(1000, random_state=1).explore(
        column='quadrant_origin',
        categorical=True,
        tiles="CartoDB positron",
        style_kwds={'weight': 1, 'opacity': 0.5}
    )
    return (LineString,)


@app.cell
def _(city_pairs_diff_quadrant, pd):
    # Create a simplified dataframe with only ids and lat/lon positions
    city_pairs_export = pd.DataFrame({
        'id_origin': city_pairs_diff_quadrant['id_origin'],
        'id_dest': city_pairs_diff_quadrant['id_dest'],
        'lat_origin': city_pairs_diff_quadrant['geometry_origin'].centroid.y,
        'lon_origin': city_pairs_diff_quadrant['geometry_origin'].centroid.x,
        'lat_dest': city_pairs_diff_quadrant['geometry_dest'].centroid.y,
        'lon_dest': city_pairs_diff_quadrant['geometry_dest'].centroid.x
    })

    # Export to parquet
    # city_pairs_export.to_parquet('city_pairs_diff_quadrant.parquet', index=False)

    city_pairs_export.head()
    return (city_pairs_export,)


@app.cell
def _(city_pairs_export):
    berlin_id = '9187e609-5a2f-4535-85ec-e2b88399eea3'
    berlin_city_pairs_export = city_pairs_export[city_pairs_export['id_origin'] == berlin_id]

    # berlin_city_pairs_export.to_parquet('berlin_city_pairs_diff_quadrant.parquet', index=False)

    berlin_city_pairs_export.head()
    return (berlin_city_pairs_export,)


@app.cell
def _(berlin_city_pairs_export):
    berlin_city_pairs_export.info()
    return


@app.cell
def _(pd):
    travel_times = pd.read_parquet('berlin_city_pairs_diff_quadrant_travel_times.parquet')
    travel_times.head()
    return (travel_times,)


@app.cell
def _(travel_times):
    travel_times[(travel_times['success'] == True) & (travel_times['total_time'] != 0)]
    return


@app.cell
def _(LineString, gpt, travel_times):
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
    return


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

    travel_routes_gdf.explore(
        column='total_time',
        cmap='RdYlGn_r',
        tiles="CartoDB positron",
        style_kwds={'weight': 2, 'opacity': 0.7},
        tooltip=['id_origin', 'id_dest', 'total_time']
    )
    return


if __name__ == "__main__":
    app.run()
