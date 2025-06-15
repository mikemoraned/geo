SELECT *
FROM division_area
WHERE names.primary LIKE '%Venice%'

---

WITH city AS (           
    SELECT geometry
    FROM   division_area
    WHERE  
    	-- id = '0851d7537fffffff015d4f9f0d8b31e6' -- Berlin
    	-- id = '085ba2a9bfffffff01a888f06236016b' -- Edinburgh
    	id = '0850a811bfffffff010c628b3d6d6ee3' -- Amsterdam
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

-- Berlin:
-- 100 * 0.00706335930785659 / 0.11792945786744011 = 5.9894783166
-- Edinburgh:
-- 100 * 0.0003313394412799 / 0.03784896591359801 = 0.8754253473
-- Amsterdam:
-- 100 * 0.004743008080688963 / 0.032147921495904924 = 14.7537005815

---

-- all elements (not just polygons and multipolygons)
WITH city AS (           
    SELECT geometry
    FROM   division_area
    WHERE  
    	-- id = '0851d7537fffffff015d4f9f0d8b31e6' -- Berlin
    	id = '085ba2a9bfffffff01a888f06236016b' -- Edinburgh
    	-- id = '0850a811bfffffff010c628b3d6d6ee3' -- Amsterdam
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
)
SELECT *
FROM   base_water        AS w
CROSS  JOIN bbox         AS bb
WHERE  (w.bbox).xmin > bb.xmin   
  AND  (w.bbox).ymin > bb.ymin
  AND  (w.bbox).xmax < bb.xmax
  AND  (w.bbox).ymax < bb.ymax
