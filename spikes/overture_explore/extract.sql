INSTALL spatial;
LOAD spatial;

CREATE TABLE division_area_subset AS
SELECT
    *
FROM
    read_parquet('s3://overturemaps-us-west-2/release/2025-02-19.0/theme=divisions/type=division_area/*', hive_partitioning=1)
WHERE
    subtype IN ['county','locality']

-- overturemaps download -f geoparquet --type=water -o water_all.parquet
CREATE OR REPLACE TABLE base_water AS
SELECT
    *
FROM
    read_parquet('/Users/mxm/Code/mine/geo/spikes/overture_explore/water_all.parquet')