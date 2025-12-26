import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import geopandas as gpd
    import pandas as pd
    import overturemaps
    import marimo as mo
    import duckdb
    from shapely import wkt, LineString
    import seaborn as sns
    return LineString, duckdb, gpd, mo, pd, sns, wkt


@app.cell
def _(mo):
    mo.md(r"""
    # Load Overture Dataset Subsets
    """)
    return


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
    def load_german_cities(conn, release="2025-12-17.0"):
        query = f"""
        SELECT * EXCLUDE(geometry), ST_AsText(geometry) AS geometry 
        FROM read_parquet('s3://overturemaps-us-west-2/release/{release}/theme=divisions/type=division/*', filename=true, hive_partitioning=1)
        WHERE country = 'DE' AND class = 'city'
        """
        return query_to_dataframe(conn, query)
    return (load_german_cities,)


@app.cell
def _(query_to_dataframe):
    def load_germany_boundary(conn, release="2025-12-17.0"):
        query = f"""
        SELECT * EXCLUDE(geometry), ST_AsText(geometry) AS geometry 
        FROM read_parquet('s3://overturemaps-us-west-2/release/{release}/theme=divisions/type=division_area/*', filename=true, hive_partitioning=1)
        WHERE country = 'DE' 
              AND subtype='country' 
              AND class = 'land'
        LIMIT 1
        """
        return query_to_dataframe(conn, query)
    return (load_germany_boundary,)


@app.cell
def _(duckdb_connection):
    conn = duckdb_connection()
    return (conn,)


@app.cell
def _(conn, load_german_cities):
    cities = load_german_cities(conn)
    cities.head()
    return (cities,)


@app.cell
def _(conn, load_germany_boundary):
    germany_boundary = load_germany_boundary(conn)
    germany_boundary.head()
    return (germany_boundary,)


@app.cell
def _(cities, germany_boundary):
    germany_plot = germany_boundary.explore(
        tiles="CartoDB positron", color="lightgray"
    )
    cities.explore(m=germany_plot, color="red")
    germany_plot
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # Assign cities to quadrants

    Split cities into North/South and East/West relative to center of Germany
    """)
    return


@app.function
def assign_to_quadrants(cities, germany):
    assigned = cities.copy()

    # Get the centroid of Germany
    centroid = germany.geometry.centroid.iloc[0]

    # Determine North/South position based on
    # latitude relative to centroid

    assigned["ns"] = assigned.geometry.centroid.y.apply(
        lambda y: "North" if y > centroid.y else "South"
    )

    # Determine East/West position based on
    # longitude relative to centroid
    assigned["ew"] = assigned.geometry.centroid.x.apply(
        lambda x: "East" if x > centroid.x else "West"
    )

    # Combine into a single position label
    assigned["quadrant"] = assigned["ns"] + "-" + assigned["ew"]

    return assigned


@app.cell
def _(cities, germany_boundary):
    cities_quadrants = assign_to_quadrants(cities, germany_boundary)
    return (cities_quadrants,)


@app.cell
def _(cities_quadrants, germany_boundary):
    quadrant_plot = germany_boundary.explore(
        tiles="CartoDB positron", color="lightgray"
    )
    cities_quadrants.explore("quadrant", m=quadrant_plot, categorical=True)
    quadrant_plot
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # Assemble routes

    We only want to examine routes that transition North<->South or East<->West but not, for example across from North-East to South-West.
    """)
    return


@app.function
def all_possible_routes(cities_quadrants):
    possible = cities_quadrants.copy()
    # all pairs
    paired = possible.merge(
        possible, how="cross", suffixes=("_origin", "_dest")
    )
    # exclude route to self and within same quadrant
    paired = paired[
        (paired["id_origin"] != paired["id_dest"])
        & (paired["quadrant_origin"] != paired["quadrant_dest"])
    ]
    paired["route_label"] = (
        paired["quadrant_origin"] + "->" + paired["quadrant_dest"]
    )
    return paired


@app.cell
def _(LineString, gpd):
    def visualise_route_labels(paired):
        # Create line geometries between origin and destination cities
        lines = paired.copy()
        lines["geometry"] = paired.apply(
            lambda row: LineString([row["geometry_origin"], row["geometry_dest"]]),
            axis=1,
        )
        return gpd.GeoDataFrame(lines, geometry="geometry", crs="EPSG:4326")
    return (visualise_route_labels,)


@app.cell
def _(cities_quadrants, visualise_route_labels):
    visualise_route_labels(all_possible_routes(cities_quadrants)).sample(
        100, random_state=1
    ).explore("route_label", tiles="CartoDB positron")
    return


@app.function
def filter_to_allowed_routes(paired, allowed_route_labels):
    return paired[paired["route_label"].isin(allowed_route_labels)]


@app.cell
def _(cities_quadrants):
    routes = filter_to_allowed_routes(
        all_possible_routes(cities_quadrants),
        allowed_route_labels=[
            "North-East->South-East",
            "South-East->North-East",
            "North-West->South-West",
            "South-West->North-West",
            "North-West->North-East",
            "North-East->North-West",
            "South-West->South-East",
            "South-East->South-West",
        ],
    )
    return (routes,)


@app.cell
def _(routes, visualise_route_labels):
    visualise_route_labels(routes).sample(300, random_state=2).explore(
        "route_label", tiles="CartoDB positron"
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    # Export / Import from travel time lookup
    """)
    return


@app.cell
def _(pd):
    def export_simplified(df, filename):
        for_export = pd.DataFrame(
            {
                "id_origin": df["id_origin"],
                "id_dest": df["id_dest"],
                "lat_origin": df["geometry_origin"].centroid.y,
                "lon_origin": df["geometry_origin"].centroid.x,
                "lat_dest": df["geometry_dest"].centroid.y,
                "lon_dest": df["geometry_dest"].centroid.x,
            }
        )
        for_export.to_parquet(filename, index=False)
    return (export_simplified,)


@app.cell
def _(export_simplified, routes):
    export_simplified(routes, filename="routes.parquet")
    return


@app.cell
def _(LineString, gpd, pd):
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
                coords = [
                    (lon, lat)
                    for lat, lon in polyline.decode(encoded_polyline, precision=6)
                ]
                if len(coords) >= 2:
                    lines.append(LineString(coords))
        if len(lines) == 0:
            return None
        return MultiLineString(lines)


    def import_travel_times(filename):
        travel_times = pd.read_parquet(filename)

        # Create new geodataframe with decoded route geometries
        travel_times_routes = travel_times[
            (travel_times["success"] == True)
        ].copy()
        travel_times_routes["route_geometry"] = travel_times_routes[
            "polylines"
        ].apply(decode_polylines_to_multilinestring)

        return gpd.GeoDataFrame(
            travel_times_routes.drop(columns=["polylines"]),
            geometry="route_geometry",
            crs="EPSG:4326",
        )
    return (import_travel_times,)


@app.cell
def _(import_travel_times):
    travel_times = import_travel_times("routes_travel_times.parquet")
    travel_times.head()
    return (travel_times,)


@app.cell
def _(travel_times):
    import lonboard
    from lonboard import Map

    # Create a PathLayer for the route geometries
    path_layer = lonboard.PathLayer.from_geopandas(
        travel_times,
        width_min_pixels=2,
    )

    Map(layers=[path_layer], basemap_style=lonboard.basemap.CartoBasemap.Positron)
    return


@app.cell
def _(sns, travel_times):
    sns.kdeplot(data=travel_times, x="total_time")
    return


@app.cell
def _(sns, travel_times):
    sns.histplot(data=travel_times, x="total_time")
    return


@app.cell
def _(routes, travel_times):
    travel_times_with_routes = travel_times.merge(
        routes,
        on=["id_origin", "id_dest"],
        how="left"
    )
    travel_times_with_routes
    return (travel_times_with_routes,)


@app.cell
def _(sns, travel_times_with_routes):
    sns.histplot(data=travel_times_with_routes, x="total_time", hue="route_label")
    return


@app.cell
def _(sns, travel_times_with_routes):
    sns.kdeplot(data=travel_times_with_routes, x="total_time", hue="route_label")
    return


@app.cell
def _(travel_times_with_routes):
    travel_times_with_route_category = travel_times_with_routes.copy()
    travel_times_with_route_category['category'] = travel_times_with_route_category.route_label.apply(
            lambda r: "North<->South" if r in [
                "North-East->South-East",
                "South-East->North-East",
                "North-West->South-West",
                "South-West->North-West",
            ] else "East<->West"
        )
    return (travel_times_with_route_category,)


@app.cell
def _(sns, travel_times_with_route_category):
    sns.kdeplot(data=travel_times_with_route_category, x="total_time", hue="category")
    return


@app.cell
def _(sns, travel_times_with_route_category):
    sns.histplot(data=travel_times_with_route_category, x="total_time", hue="category")
    return


@app.cell
def _(sns, travel_times_with_route_category):
    sns.kdeplot(data=travel_times_with_route_category, x="total_time", hue="category", common_norm=False)
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    need to normalise time by crow-flies distance (as otherwise I am maybe just measuring differences in distances not times)
    """)
    return


@app.cell
def _(travel_times_with_route_category):
    from geopy.distance import geodesic

    # Calculate crow-flies distance between origin and destination
    travel_times_with_distance = travel_times_with_route_category.copy()
    travel_times_with_distance['crow_flies_km'] = travel_times_with_distance.apply(
        lambda row: geodesic(
            (row['lat_origin'], row['lon_origin']),
            (row['lat_dest'], row['lon_dest'])
        ).kilometers,
        axis=1
    )

    travel_times_with_distance[['id_origin', 'id_dest', 'total_time', 'crow_flies_km', 'category']].head()
    return (travel_times_with_distance,)


@app.cell
def _(travel_times_with_distance):
    # Calculate speed as crow-flies distance divided by total time
    travel_times_with_speed = travel_times_with_distance.copy()
    travel_times_with_speed['speed_km_per_sec'] = travel_times_with_speed['crow_flies_km'] / travel_times_with_speed['total_time']

    # Also add speed in km/h for more intuitive interpretation
    travel_times_with_speed['speed_km_per_h'] = travel_times_with_speed['speed_km_per_sec'] * 3600

    travel_times_with_speed[['id_origin', 'id_dest', 'total_time', 'crow_flies_km', 'speed_km_per_sec', 'speed_km_per_h', 'category']].head()
    return (travel_times_with_speed,)


@app.cell
def _(sns, travel_times_with_speed):
    sns.kdeplot(data=travel_times_with_speed, x="speed_km_per_h", hue="category", common_norm=False)
    return


@app.cell
def _(sns, travel_times_with_speed):
    sns.histplot(data=travel_times_with_speed, x="speed_km_per_h", hue="category")
    return


@app.cell
def _(sns, travel_times_with_speed):
    sns.kdeplot(data=travel_times_with_speed, x="crow_flies_km", hue="category", common_norm=False)
    return


if __name__ == "__main__":
    app.run()
