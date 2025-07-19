LOAD spatial;

CREATE OR REPLACE VIEW land_cover AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=base/type=land_cover/*', 
                 hive_partitioning=1)
                 
SELECT *
FROM land_cover
LIMIT 100

CREATE OR REPLACE VIEW land AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=base/type=land/*', 
                 hive_partitioning=1)
                 
SELECT *
FROM land
LIMIT 100

CREATE OR REPLACE VIEW division_area AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=divisions/type=division_area/*', 
                 hive_partitioning=1)

SELECT *
FROM division_area
LIMIT 100
                 
SELECT subtype, COUNT(1)
FROM land_cover
GROUP BY subtype
ORDER BY COUNT(1) DESC

SELECT subtype, COUNT(1)
FROM land
GROUP BY subtype
ORDER BY COUNT(1) DESC

SELECT subtype, class, COUNT(1)
FROM land
GROUP BY subtype, class
ORDER BY COUNT(1) DESC

SET VARIABLE city_gers = '58a34fa4-bc76-476e-81a8-1ed8a5cd693f';

SET VARIABLE city_bounds = (SELECT geometry FROM division_area WHERE id = getvariable('city_gers'));

SELECT getvariable('city_bounds')

WITH city_bounds AS (
	SELECT geometry FROM division_area WHERE id = getvariable('city_gers')
)
SELECT *
FROM land_cover lc JOIN city_bounds cb ON ST_Intersects(lc.geometry, cb.geometry)
LIMIT 100

SELECT *
FROM division_area
WHERE id = getvariable('city_gers')

SELECT *
FROM land_cover 
WHERE bbox.xmin >= -3.4495325 AND bbox.xmax <= -3.077676
      AND bbox.ymin >= 55.819736 AND bbox.ymax <= 56.001587

      
WITH edinburgh_land_cover AS (
	SELECT *
    FROM land_cover 
    WHERE bbox.xmin >= -3.4495325 AND bbox.xmax <= -3.077676
          AND bbox.ymin >= 55.819736 AND bbox.ymax <= 56.001587
)
SELECT subtype, COUNT(1)
FROM edinburgh_land_cover
GROUP BY subtype
ORDER BY COUNT(1) DESC

WITH edinburgh_land_cover AS (
	SELECT *
    FROM land_cover 
    WHERE bbox.xmin >= -3.4495325 AND bbox.xmax <= -3.077676
          AND bbox.ymin >= 55.819736 AND bbox.ymax <= 56.001587
)
SELECT *
FROM edinburgh_land_cover
WHERE subtype IN (
	'shrub',
	'forest'
)

WITH edinburgh_land AS (
	SELECT *
	FROM land 
	WHERE bbox.xmin >= -3.4495325 AND bbox.xmax <= -3.077676
	      AND bbox.ymin >= 55.819736 AND bbox.ymax <= 56.001587
)
SELECT subtype, class, COUNT(1)
FROM edinburgh_land
GROUP BY subtype, class
ORDER BY COUNT(1) DESC

WITH edinburgh_land AS (
	SELECT *
	FROM land 
	WHERE bbox.xmin >= -3.4495325 AND bbox.xmax <= -3.077676
	      AND bbox.ymin >= 55.819736 AND bbox.ymax <= 56.001587
), green AS (
    SELECT *
    FROM (
        VALUES
            ('tree', 'tree'),
            ('grass','meadow')
    ) AS t(subtype, class)
)

SELECT *
FROM edinburgh_land
WHERE concat_ws(subtype,class) IN (SELECT concat_ws(subtype,class) FROM green)
