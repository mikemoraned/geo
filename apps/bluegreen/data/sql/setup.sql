LOAD SPATIAL;

CREATE OR REPLACE VIEW division_area AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-03-19.0/theme=divisions/type=division_area/*', 
                 hive_partitioning=1)

SELECT *
FROM division_area
LIMIT 10

CREATE OR REPLACE VIEW base_water AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-03-19.0/theme=base/type=water/*', 
                 hive_partitioning=1)
                 
SELECT *
FROM base_water
LIMIT 10