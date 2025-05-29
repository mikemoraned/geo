LOAD SPATIAL;

CREATE VIEW division_area AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-03-19.0/theme=divisions/type=division_area/*', 
                 hive_partitioning=1)

SELECT *
FROM division_area
LIMIT 10

SELECT *
FROM division_area
WHERE id = '085ba2a9bfffffff01a888f06236016b'