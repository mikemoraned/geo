LOAD spatial;

CREATE OR REPLACE VIEW division_area AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=divisions/type=division_area/*')
                 
CREATE OR REPLACE VIEW land_cover AS
SELECT
    *
FROM
    read_parquet('/Volumes/PRO-G40/OvertureMaps/data/release/2025-06-25.0/theme=base/type=land_cover/*')

-- save as GeoJSON to make it easier to see what it looks like in geojson.io
COPY (
	SELECT id, geometry -- can't save the full thing vai GDAL as it doesn't support the STRUCT
	FROM division_area
	WHERE id IN (
		'dbd84987-2831-4b62-a0e0-a3f3d5a237c2' -- amsterdam
	)
) 
TO '/Users/mxm/Code/mine/geo/shared/geo-overturemaps/tests/data/release/2025-06-25.0/division_area.geojson'
WITH (FORMAT gdal, DRIVER 'GeoJSON')


-- save data for use in tests
COPY (
	SELECT *
	FROM division_area
	WHERE id IN (
		'dbd84987-2831-4b62-a0e0-a3f3d5a237c2' -- amsterdam
	)
) 
TO '/Users/mxm/Code/mine/geo/shared/geo-overturemaps/tests/data/release/2025-06-25.0/theme=divisions/type=division_area/data.ztd.parquet'
WITH (FORMAT parquet, COMPRESSION 'zstd')

COPY (
	SELECT *
	FROM land_cover
	WHERE -- amsterdam area
		  bbox.xmin >= 4.728777 AND bbox.xmax <= 5.107681
		  AND bbox.ymin >= 52.277977 AND bbox.ymax <= 52.431064
) 
TO '/Users/mxm/Code/mine/geo/shared/geo-overturemaps/tests/data/release/2025-06-25.0/theme=base/type=land_cover/data.ztd.parquet'
WITH (FORMAT parquet, COMPRESSION 'zstd')

-- load in again to confirm it worked
SELECT
    *
FROM
    read_parquet('/Users/mxm/Code/mine/geo/shared/geo-overturemaps/tests/data/release/2025-06-25.0/theme=divisions/type=division_area/*', 
                 hive_partitioning=1)