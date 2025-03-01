# Finding city boundaries

Looking at overturemaps, which is likely just inheriting this structure, 
a 'city' can be different subtypes of "division_area". For example, "Amsterdam" is a
"locality" whereas "Edinburgh" is "county".

Hmm, though "Amsterdam" appears multiple times, and also in different types? (county and locality)


## Getting subset

Need to do this for `geometry` column to be interpreted (otherwise shows, or is downloaded, as a `BLOB`)
```aiignore
INSTALL spatial;
LOAD spatial;
```

Note that if you get stuck with a `BLOB` you can re-interpret it into geo e.g.
```aiignore
SELECT
ST_GeomFromWKB(geometry) AS geometry2,
*
FROM division_area_subset
LIMIT 10
```

Download subset (took about 15 minutes running from home connection)
```sql
CREATE TABLE division_area_subset AS
SELECT
    *
FROM
    read_parquet('s3://overturemaps-us-west-2/release/2025-01-22.0/theme=divisions/type=division_area/*', hive_partitioning=1)
WHERE
    subtype IN ['county','locality']
    AND country IN ['NL','GB']
```

Find Amsterdam or Edinburgh places:
```aiignore
FROM division_area_subset
WHERE names.primary LIKE '%sterdam%' OR names.primary LIKE '%Edinburgh%'
```

## Misc queries

```aiignore
WITH areas AS (
  SELECT ST_Area(geometry) AS area, *
  FROM division_area_subset
)
-- area of Edinburgh:
-- SELECT *
-- FROM areas
-- WHERE id = '085ba2a9bfffffff01a888f06236016b'
-- = 0.0378489675480854
-- area of Amsterdam:
-- SELECT *
-- FROM areas
-- WHERE id = '0850a811bfffffff010c628b3d6d6ee3'
-- = 0.03214792149590503
FROM areas
WHERE area BETWEEN 0.03214792149590503 AND 0.0378489675480854
```

links:
* https://docs.overturemaps.org/schema/reference/divisions/division_area/

# Interesting stuff found in passing

https://kepler.gl/demo accepts 

DBeaver is nice, as it has builtin geo support

https://sedona.apache.org/latest/

paris: https://spelunker.whosonfirst.org/id/1159322569/