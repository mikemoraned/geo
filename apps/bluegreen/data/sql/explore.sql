LOAD SPATIAL;

SELECT *
FROM read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-03-19.0/theme=transportation/type=segment/*', hive_partitioning=1)
WHERE subtype='rail'
LIMIT 100

SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-03-19.0/theme=divisions/type=division_area/*', hive_partitioning=1)
WHERE
    subtype IN ['county','locality']
    AND country IN ['NL','GB']