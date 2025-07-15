LOAD spatial;

CREATE OR REPLACE VIEW gers_registry AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/registry/*', hive_partitioning=1)
                 
SELECT *
FROM gers_registry
LIMIT 100

SELECT *
FROM gers_registry
WHERE id IN ('085ba2a9bfffffff01a888f06236016b')



CREATE OR REPLACE VIEW division_area_june_to_may_id_mapping AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/june_to_may_id_mapping/theme=divisions/type=division_area/*', hive_partitioning=1)
    
SELECT *
FROM division_area_june_to_may_id_mapping
LIMIT 100

SELECT *
FROM division_area_june_to_may_id_mapping
WHERE previous_id IN (
	'0850a811bfffffff010c628b3d6d6ee3', -- amsterdam
	'0851d7537fffffff015d4f9f0d8b31e6', -- berlin
	'085ba2a9bfffffff01a888f06236016b', -- edinburgh
	'0850609cbfffffff01721b0295fac0f6', -- gothemburg
	'08596a323fffffff01c67019d4f1c8d0', -- hamburg
	'08b2db2b70d8bfff0005d68bbb7ae2f8', -- jerusalem
	'0856cf4d7fffffff014d3ee666c435a5', -- new_york
	'085f1366ffffffff0123ad1610d936d7', -- paris
	'085342877fffffff013156f250ee4659', -- ronneburg
	'0854ae143fffffff01609212ad48701b', -- stockholm
	'085d465cbfffffff01b558002f798928', -- vejle
)

WITH city_values AS (
    SELECT *
    FROM (
        VALUES
            ('0850a811bfffffff010c628b3d6d6ee3', 'amsterdam'),
            ('0851d7537fffffff015d4f9f0d8b31e6', 'berlin'),
            ('085ba2a9bfffffff01a888f06236016b', 'edinburgh'),
            ('0850609cbfffffff01721b0295fac0f6', 'gothemburg'),
            ('08596a323fffffff01c67019d4f1c8d0', 'hamburg'),
            ('08b2db2b70d8bfff0005d68bbb7ae2f8', 'jerusalem'),
            ('0856cf4d7fffffff014d3ee666c435a5', 'new_york'),
            ('085f1366ffffffff0123ad1610d936d7', 'paris'),
            ('085342877fffffff013156f250ee4659', 'ronneburg'),
            ('0854ae143fffffff01609212ad48701b', 'stockholm'),
            ('085d465cbfffffff01b558002f798928', 'vejle')
    ) AS t(value, city)
)

-- Example usage:
SELECT *
FROM city_values
ORDER BY city ASC

WITH city_values AS (
    SELECT *
    FROM (
        VALUES
            ('0850a811bfffffff010c628b3d6d6ee3', 'amsterdam'),
            ('0851d7537fffffff015d4f9f0d8b31e6', 'berlin'),
            ('085ba2a9bfffffff01a888f06236016b', 'edinburgh'),
            ('0850609cbfffffff01721b0295fac0f6', 'gothemburg'),
            ('08596a323fffffff01c67019d4f1c8d0', 'hamburg'),
            ('08b2db2b70d8bfff0005d68bbb7ae2f8', 'jerusalem'),
            ('0856cf4d7fffffff014d3ee666c435a5', 'new_york'),
            ('085f1366ffffffff0123ad1610d936d7', 'paris'),
            ('085342877fffffff013156f250ee4659', 'ronneburg'),
            ('0854ae143fffffff01609212ad48701b', 'stockholm'),
            ('085d465cbfffffff01b558002f798928', 'vejle')
    ) AS t(id, city)
)

SELECT c.city, m.id, m.previous_id
FROM division_area_june_to_may_id_mapping m JOIN city_values c ON m.previous_id = c.id
ORDER BY c.city ASC

-- |city      |id                                  |previous_id                     |
-- |----------|------------------------------------|--------------------------------|
-- |amsterdam |dbd84987-2831-4b62-a0e0-a3f3d5a237c2|0850a811bfffffff010c628b3d6d6ee3|
-- |berlin    |5d231bd3-41ee-4aed-84b2-e3c609063672|0851d7537fffffff015d4f9f0d8b31e6|
-- |edinburgh |58a34fa4-bc76-476e-81a8-1ed8a5cd693f|085ba2a9bfffffff01a888f06236016b|
-- |gothemburg|6ef8f90c-4f3b-4d59-92a5-3306f96e9d4c|0850609cbfffffff01721b0295fac0f6|
-- |hamburg   |955c0b4a-b28c-401f-827c-7d0837ba8104|08596a323fffffff01c67019d4f1c8d0|
-- |new_york  |5dcf6d7a-6e81-4795-87fb-26f6d67644ed|0856cf4d7fffffff014d3ee666c435a5|
-- |paris     |4e5c3982-82ce-43ba-aef2-6b501d542604|085f1366ffffffff0123ad1610d936d7|
-- |ronneburg |32883d55-acaa-492e-94eb-fe8335ee5169|085342877fffffff013156f250ee4659|
-- |stockholm |d8ce38d3-c16e-4ec4-9e6f-5ab76bbb4d0c|0854ae143fffffff01609212ad48701b|
-- |vejle     |706a6083-00dd-4aa8-b530-c4160b05c0b7|085d465cbfffffff01b558002f798928|

CREATE OR REPLACE VIEW base_land_cover_june_to_may_id_mapping AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/june_to_may_id_mapping/theme=base/type=land_cover/*', hive_partitioning=1)
    
SELECT *
FROM base_land_cover_june_to_may_id_mapping
LIMIT 100


CREATE OR REPLACE VIEW base_land_cover_may AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-05-21.0/theme=base/type=land_cover/*', 
                 hive_partitioning=1)
                 
CREATE OR REPLACE VIEW base_land_cover_june AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=base/type=land_cover/*', 
                 hive_partitioning=1)

SELECT *
FROM base_land_cover_may
WHERE id='08b2db2b70d8bfff0005d68bbb7ae2f8'

SELECT *
FROM base_land_cover_may
WHERE bbox.xmin >= 35.156044 AND bbox.xmax <= 35.507954
      AND bbox.ymin >= 31.656881 AND bbox.ymax <= 31.952284
      AND id='08b2db2b70d8bfff0005d68bbb7ae2f8'

SELECT *
FROM base_land_cover_june
WHERE bbox.xmin >= 35.156044 AND bbox.xmax <= 35.507954
      AND bbox.ymin >= 31.656881 AND bbox.ymax <= 31.952284


SELECT id,geometry
FROM base_land_cover_may
WHERE id='08b2db2b70d8bfff0005d68bbb7ae2f8'
UNION ALL
SELECT id,geometry
FROM base_land_cover_june
WHERE bbox.xmin >= 35.156044 AND bbox.xmax <= 35.507954
      AND bbox.ymin >= 31.656881 AND bbox.ymax <= 31.952284
      
CREATE OR REPLACE VIEW division_area_june AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=divisions/type=division_area/*', 
                 hive_partitioning=1)

SELECT *
FROM division_area_june
WHERE bbox.xmin >= 35.156044 AND bbox.xmax <= 35.507954
      AND bbox.ymin >= 31.656881 AND bbox.ymax <= 31.952284
      
      
SELECT *
FROM base_land_cover_june
WHERE id='e77650e6-4bc3-5e54-b5b4-46f8e3b1b375'

SELECT *
FROM base_land_cover_june
WHERE bbox.xmin >= 35.156044 AND bbox.xmax <= 35.507954
      AND bbox.ymin >= 31.656881 AND bbox.ymax <= 31.952284
      
SELECT *
FROM base_land_cover_june
WHERE bbox.xmin >= 35.156044 AND bbox.xmax <= 35.507954
      AND bbox.ymin >= 31.656881 AND bbox.ymax <= 31.952284
      AND id='e77650e6-4bc3-5e54-b5b4-46f8e3b1b375'