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
    

SELECT *
FROM division_area
WHERE id = '085ba2a9bfffffff01a888f06236016b' -- Edinburgh

SELECT *
FROM division_area
WHERE id = '0851d7537fffffff015d4f9f0d8b31e6' -- Berlin

SELECT bbox.*,*
FROM division_area
WHERE id = '0851d7537fffffff015d4f9f0d8b31e6' -- Berlin

SELECT *
FROM base_water
WHERE bbox.xmin > 2.276
      AND bbox.ymin > 48.865
      AND bbox.xmax < 2.314
      AND bbox.ymax < 48.882
      
SELECT *
FROM base_water
WHERE bbox.xmin > 13.088345
      AND bbox.ymin > 52.338238
      AND bbox.xmax < 13.761163
      AND bbox.ymax < 52.67551
      
EXPLAIN
SELECT *
FROM base_water
WHERE bbox.xmin > 13.088345
      AND bbox.ymin > 52.338238
      AND bbox.xmax < 13.761163
      AND bbox.ymax < 52.67551

SELECT subtype, class, COUNT(1)
FROM base_water
WHERE bbox.xmin > 13.088345
      AND bbox.ymin > 52.338238
      AND bbox.xmax < 13.761163
      AND bbox.ymax < 52.67551
GROUP BY subtype, class

SELECT ST_GeometryType(geometry) AS t, COUNT(1)
FROM base_water
WHERE bbox.xmin > 13.088345
      AND bbox.ymin > 52.338238
      AND bbox.xmax < 13.761163
      AND bbox.ymax < 52.67551
GROUP BY 1

SELECT *
FROM base_water
WHERE bbox.xmin > 13.088345
      AND bbox.ymin > 52.338238
      AND bbox.xmax < 13.761163
      AND bbox.ymax < 52.67551
      AND ST_GeometryType(geometry) IN ('POLYGON','MULTIPOLYGON')

      
SELECT ST_Union_Agg(geometry)
FROM base_water
WHERE bbox.xmin > 13.088345
      AND bbox.ymin > 52.338238
      AND bbox.xmax < 13.761163
      AND bbox.ymax < 52.67551
      AND ST_GeometryType(geometry) IN ('POLYGON','MULTIPOLYGON')
    
WITH unioned AS (
	SELECT geometry
	FROM division_area
	WHERE id = '0851d7537fffffff015d4f9f0d8b31e6'
UNION ALL
	SELECT ST_Union_Agg(geometry) AS geometry
	FROM base_water
	WHERE bbox.xmin > 13.088345
	      AND bbox.ymin > 52.338238
	      AND bbox.xmax < 13.761163
	      AND bbox.ymax < 52.67551
	      AND ST_GeometryType(geometry) IN ('POLYGON','MULTIPOLYGON')
)
SELECT ST_Intersection_Agg(geometry)
FROM unioned

WITH berlin AS (
	SELECT geometry
	FROM division_area
	WHERE id = '0851d7537fffffff015d4f9f0d8b31e6'
), unioned AS (
	SELECT * FROM berlin
UNION ALL
	SELECT ST_Union_Agg(geometry) AS geometry
	FROM base_water
	WHERE bbox.xmin > 13.088345
	      AND bbox.ymin > 52.338238
	      AND bbox.xmax < 13.761163
	      AND bbox.ymax < 52.67551
	      AND ST_GeometryType(geometry) IN ('POLYGON','MULTIPOLYGON')
)
SELECT ST_Intersection_Agg(geometry)
FROM unioned
UNION ALL
SELECT * FROM berlin

WITH berlin AS (           -- 1. get the Berlin polygon once
    SELECT d.geometry
    FROM   division_area AS d
    WHERE  d.id = '0851d7537fffffff015d4f9f0d8b31e6'
),

bbox AS (                  -- 2. pull out its bounding-box numbers
    SELECT
        ST_XMin(env) AS xmin,
        ST_YMin(env) AS ymin,
        ST_XMax(env) AS xmax,
        ST_YMax(env) AS ymax
    FROM (
        SELECT ST_Envelope(b.geometry) AS env   -- ST_Envelope returns a BOX geometry
        FROM   berlin AS b
    ) s
),

unioned AS (               -- 3. keep Berlin *plus* union of all water that lies inside its bbox
    SELECT b.geometry                         -- (a) Berlin itself
    FROM   berlin AS b

    UNION ALL

    SELECT ST_Union_Agg(w.geometry) AS geometry  -- (b) water polygons inside the bbox
    FROM   base_water        AS w
    CROSS  JOIN bbox         AS bb
    WHERE  ST_GeometryType(w.geometry) IN ('POLYGON','MULTIPOLYGON')
      AND  (w.bbox).xmin > bb.xmin   -- compare the pre-computed bbox columns
      AND  (w.bbox).ymin > bb.ymin
      AND  (w.bbox).xmax < bb.xmax
      AND  (w.bbox).ymax < bb.ymax
)

-- 4. aggregate the intersection and return Berlin once more
SELECT ST_Intersection_Agg(u.geometry)
FROM   unioned AS u

UNION ALL
SELECT b.geometry
FROM   berlin AS b;

---

WITH city AS (           
    SELECT geometry
    FROM   division_area
    WHERE  id = '085ba2a9bfffffff01a888f06236016b'
),
bbox AS (                  
    SELECT
        ST_XMin(env) AS xmin,
        ST_YMin(env) AS ymin,
        ST_XMax(env) AS xmax,
        ST_YMax(env) AS ymax
    FROM (
        SELECT ST_Envelope(geometry) AS env
        FROM   city
    )
),
unioned AS (               
    SELECT geometry                         
    FROM   city
    UNION ALL
    SELECT ST_Union_Agg(w.geometry) AS geometry  
    FROM   base_water        AS w
    CROSS  JOIN bbox         AS bb
    WHERE  ST_GeometryType(w.geometry) IN ('POLYGON','MULTIPOLYGON')
      AND  (w.bbox).xmin > bb.xmin   
      AND  (w.bbox).ymin > bb.ymin
      AND  (w.bbox).xmax < bb.xmax
      AND  (w.bbox).ymax < bb.ymax
), intersection AS (
	SELECT ST_Intersection_Agg(geometry) AS geometry 
	FROM   unioned
)
SELECT geometry, ST_Area(geometry) AS area
FROM   intersection
UNION ALL
SELECT geometry, ST_Area(geometry) AS area
FROM   city

-- 100 * 0.0003313394412799 / 0.03784896591359801 = 0.8754253473

---

WITH city AS (           
    SELECT geometry
    FROM   division_area
    WHERE  id = '0851d7537fffffff015d4f9f0d8b31e6' -- Berlin
),
bbox AS (                  
    SELECT
        ST_XMin(env) AS xmin,
        ST_YMin(env) AS ymin,
        ST_XMax(env) AS xmax,
        ST_YMax(env) AS ymax
    FROM (
        SELECT ST_Envelope(geometry) AS env
        FROM   city
    )
),
unioned AS (               
    SELECT geometry                         
    FROM   city
    UNION ALL
    SELECT ST_Union_Agg(w.geometry) AS geometry  
    FROM   base_water        AS w
    CROSS  JOIN bbox         AS bb
    WHERE  ST_GeometryType(w.geometry) IN ('POLYGON','MULTIPOLYGON')
      AND  (w.bbox).xmin > bb.xmin   
      AND  (w.bbox).ymin > bb.ymin
      AND  (w.bbox).xmax < bb.xmax
      AND  (w.bbox).ymax < bb.ymax
), intersection AS (
	SELECT ST_Intersection_Agg(geometry) AS geometry 
	FROM   unioned
)
SELECT geometry, ST_Area(geometry) AS area
FROM   intersection
UNION ALL
SELECT geometry, ST_Area(geometry) AS area
FROM   city

-- 100 * 0.00706335930785659 / 0.11792945786744011 = 5.9894783166

---

SELECT *
FROM division_area
WHERE names.primary LIKE '%Amsterdam%'

WITH city AS (           
    SELECT geometry
    FROM   division_area
    WHERE  id = '0850a811bfffffff010c628b3d6d6ee3' -- Amsterdam
),
bbox AS (                  
    SELECT
        ST_XMin(env) AS xmin,
        ST_YMin(env) AS ymin,
        ST_XMax(env) AS xmax,
        ST_YMax(env) AS ymax
    FROM (
        SELECT ST_Envelope(geometry) AS env
        FROM   city
    )
),
unioned AS (               
    SELECT geometry                         
    FROM   city
    UNION ALL
    SELECT ST_Union_Agg(w.geometry) AS geometry  
    FROM   base_water        AS w
    CROSS  JOIN bbox         AS bb
    WHERE  ST_GeometryType(w.geometry) IN ('POLYGON','MULTIPOLYGON')
      AND  (w.bbox).xmin > bb.xmin   
      AND  (w.bbox).ymin > bb.ymin
      AND  (w.bbox).xmax < bb.xmax
      AND  (w.bbox).ymax < bb.ymax
), intersection AS (
	SELECT ST_Intersection_Agg(geometry) AS geometry 
	FROM   unioned
)
SELECT geometry, ST_Area(geometry) AS area
FROM   intersection
UNION ALL
SELECT geometry, ST_Area(geometry) AS area
FROM   city

--- 100 * 0.004743008080688963 / 0.032147921495904924 = 14.7537005815

SELECT ST_CENTROID(geometry), *
FROM division_area
WHERE id = '0851d7537fffffff015d4f9f0d8b31e6'

SELECT *
FROM division_area
WHERE names.primary LIKE '%Amsterdam%'

SELECT *
FROM division_area
WHERE names.primary LIKE '%Berlin%'
      AND class = 'land'
      AND subtype = 'region'
      

---
      
      
SELECT
    *
FROM
    read_parquet('s3://overturemaps-us-west-2/release/2025-05-21.0/theme=divisions/type=division_area/*', 
                 hive_partitioning=1)
WHERE names.primary LIKE '%Venice%'

---

SELECT *
FROM ST_Read('/Users/mxm/Code/mine/geo/apps/linzer/output/amsterdam/water.geojson');


---

SELECT names.primary, *
FROM division_area
WHERE names.primary LIKE '%Martin%'

SELECT names.primary, *
FROM division_area
WHERE bbox.xmin BETWEEN 18.661508 AND 49.163027
    AND bbox.ymin BETWEEN 18.836438 AND 49.27799