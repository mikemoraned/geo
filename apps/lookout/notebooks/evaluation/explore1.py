# /// script
# dependencies = [
#     "marimo",
#     "polars[pyarrow]==2.1.0",
#     "lookout-medallion",
#     "openlayers==0.1.6",
#     "lonboard==0.17.0",
#     "geopandas==1.2.0",
#     "folium>=0.12",
#     "mapclassify==2.11.0",
#     "matplotlib==3.11.2",
# ]
# requires-python = ">=3.13"
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import pyarrow as pa
    import lookout_medallion
    import geopandas as gpd

    return gpd, lookout_medallion, pa


@app.cell
def _(lookout_medallion):
    MEDALLION_ROOT = lookout_medallion.default_root()
    MEDALLION_ROOT
    return


@app.cell
def _(lookout_medallion):
    help(lookout_medallion)
    return


@app.cell
def _(lookout_medallion, pa):
    def bronze_table(country, sql):
        return pa.table(lookout_medallion.query_bronze(sql)).drop_columns("geometry_projected")
    
    def silver_table(country, sql):
        return pa.table(lookout_medallion.query_silver(sql, country=country)).drop_columns("geometry_projected")

    return (silver_table,)


@app.cell
def _(gpd, silver_table):
    train_session_v2_gdf = gpd.GeoDataFrame.from_arrow(
        silver_table("DE","""
        SELECT *
        FROM train_session_v2
        LIMIT 10
        """)
    )
    train_session_v2_gdf
    return (train_session_v2_gdf,)


@app.cell
def _(train_session_v2_gdf):
    train_session_v2_gdf.explore()
    return


@app.cell
def _(gpd, silver_table):
    train_session_sample_v2_gdf = gpd.GeoDataFrame.from_arrow(
        silver_table("DE","""
        SELECT *
        FROM train_session_sample_v2
        ORDER BY sample_date DESC
        LIMIT 1000
        """)
    )
    train_session_sample_v2_gdf
    return (train_session_sample_v2_gdf,)


@app.cell
def _(train_session_sample_v2_gdf):
    train_session_sample_v2_gdf.explore()
    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    DESCRIBE motis_segment_v3
    """))
    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    SELECT realtime,COUNT(1)
    FROM motis_segment_v3
    GROUP BY realtime
    """))
    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    SELECT polled_date,realtime,COUNT(1)
    FROM motis_segment_v3
    GROUP BY polled_date,realtime
    ORDER BY polled_date,realtime ASC
    """))
    return


@app.cell
def _(lookout_medallion, pa):
    motis_segment_v3_table = pa.table(lookout_medallion.query_bronze("""
    SELECT *
    FROM motis_segment_v3
    """))
    motis_segment_v3_table
    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    SELECT *, CASE WHEN realtime THEN 1 ELSE 0 END AS realtime_int
    FROM motis_segment_v3
    LIMIT 1000
    """))
    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    WITH counted AS (
      SELECT polled_date,trip_id, CASE WHEN realtime THEN 1 ELSE 0 END AS realtime_int
      FROM motis_segment_v3
    )

    SELECT polled_date,COUNT(1),AVG(realtime_int) * 100 AS realtime_pct
    FROM counted
    GROUP BY polled_date
    ORDER BY polled_date ASC
    """))

    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    WITH counted AS (
      SELECT polled_date,trip_id, CASE WHEN realtime THEN 1 ELSE 0 END AS realtime_int
      FROM motis_segment_v3
    )

    SELECT trip_id,AVG(realtime_int) * 100 AS realtime_pct
    FROM counted
    GROUP BY trip_id
    HAVING realtime_pct > 0
    LIMIT 100
    """))

    return


@app.cell
def _(lookout_medallion, pa):
    pa.table(lookout_medallion.query_bronze("""
    WITH counted AS (
      SELECT polled_date,trip_id, CASE WHEN realtime THEN 1 ELSE 0 END AS realtime_int
      FROM motis_segment_v3
    ), acceptable_trips AS (
      SELECT polled_date,trip_id,AVG(realtime_int) * 100 AS realtime_pct
      FROM counted
      GROUP BY polled_date,trip_id
      HAVING realtime_pct > 90
    )

    SELECT polled_date,COUNT(1)
    FROM acceptable_trips
    GROUP BY polled_date
    ORDER BY polled_date
    """))

    return


if __name__ == "__main__":
    app.run()
