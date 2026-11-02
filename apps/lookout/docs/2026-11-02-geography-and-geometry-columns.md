# Geography and geometry columns in silver

*Assessment of 2026-11-02. It changed no stored file and ran no query against a column declared
spherical. The evidence is the store's files, this repo's code, the PostGIS, Parquet, and GeoParquet
documentation, and the source of SedonaDB 0.4.1 and geoparquet 0.7.0. Whatever survives the slice
belongs in [medallion.md](medallion.md), and the rest goes stale.*

Whether silver's two geometry columns can be named and declared as a geography and a geometry, and
whether a native encoding would pay. It is for whoever picks up the slice "geography and geometry
columns".

## The distinction

The two terms describe the surface a calculation assumes, not the coordinates alone.

- A **geography** holds angular coordinates on the round Earth, and an edge is a great-circle arc.
  [Section 4.3 of the PostGIS manual](https://postgis.net/docs/using_postgis_dbmanagement.html#PostGIS_Geography)
  is the clearest statement of the two types.
- A **geometry** holds coordinates on a plane, and an edge is a straight line. A projection always
  lands on a plane, so projected data is a geometry. A geometry need not be projected: a PostGIS
  geometry can hold SRID 4326, where
  [its distances come out in degrees](https://postgis.net/workshops/postgis-intro/geography.html).

Parquet has [a logical type for each](https://github.com/apache/parquet-format/blob/master/LogicalTypes.md).
`GEOMETRY` has planar edges, and `GEOGRAPHY` has non-planar edges and allows only a geographic CRS.
[GeoParquet 1.1](https://geoparquet.org/releases/v1.1.0/) has no such types. It has a column key,
`edges`, which is `planar` or `spherical` and defaults to `planar`.

## What silver holds

| Column | Holds | CRS | Measured with | `edges` in the file |
| --- | --- | --- | --- | --- |
| `geometry` | The geography | OGC:CRS84 | Haversine | Unset, so planar |
| `geometry_projected` | The geometry | A country's zone, such as EPSG:25832 | Straight lines | Unset, so planar |

The derivations use Haversine in `crates/motis/src/sampling.rs`, and `implied_speed` uses straight
lines in `crates/domain/src/travel.rs`. A `train_segment_v4` file states GeoParquet 1.1.0, WKB in
both columns, and `geometry` as its primary column. `wkb_field`, in `crates/medallion/src/geo.rs`,
passes no edges to `Metadata::new`.

So the code treats the two columns differently, and a file does not. The name `geometry` also points
at the wrong column.

## Declaring spherical edges

This makes a file state what the code assumes. The write looks small: `Metadata::new` takes an
`Option<Edges>`, and geoparquet 0.7.0's column metadata has an `edges` field. No test has confirmed
that the value reaches the file.

The risk is in the readers:

- **SedonaDB** carries edges in its column type, and each function states which types it accepts.
  Its source uses the geometry-only matcher 42 times and the either-type matcher 22 times, across
  `sedona-functions` and `sedona-geo`. `ST_AsBinary` accepts either. A geometry-only function on a
  spherical column is expected to find no match.
- **DuckDB, geopandas, and lonboard** were not checked.

The results can also change. A predicate over a spherical column follows great-circle edges, so
every derived dataset needs comparing before and after.

## Renaming the columns

Six datasets hold both columns: `session_v3`, `session_sample_v3`, `train_segment_v4`,
`train_session_v2`, `train_session_sample_v2`, and `water_crossing_v2`. A declaration change makes a
new version of each, and at least 18 files name the columns, including notebooks. Each dataset
re-derives from bronze, so no data needs migrating.

GeoParquet's `primary_column` key names the primary column, so a reader following that key is
unaffected. A rename alone leaves a column called `geography` that readers treat as planar, so it
follows the declaration.

## Native encodings

GeoParquet 1.1 allows GeoArrow-native columns, and geoparquet 0.7.0 writes them through
`GeoParquetWriterEncoding::GeoArrow`. SedonaDB 0.4.1 cannot read them: its `format.rs` answers any
encoding other than WKB with `Unsupported GeoParquet encoding`. Silver stays on WKB until a SedonaDB
release reads them.

Geometry takes most of the disk. These figures come from the silver of the captures to 25 October:

| Dataset | `geometry` | `geometry_projected` | Other columns |
| --- | --- | --- | --- |
| `train_segment_v4` | 1.81 GB | 3.07 GB | 0.02 GB |
| `train_session_v2` | 0.75 GB | 0.77 GB | 0.01 GB |
| `train_session_sample_v2` | 0.69 GB | 0.71 GB | 1.26 GB |

Native encoding is therefore worth measuring, though a cheaper saving can come first. The 1.70
million rows of `train_segment_v4` hold 26,965 distinct shapes, and they repeat within each file. A
Parquet dictionary over the WKB needs no change in any reader. Neither saving has been measured.

## Recommended order

1. Check each reader of the lat/lon column against a file with spherical edges.
2. If the readers allow it, declare the edges and compare every derived dataset.
3. Rename the columns, as new versions of the six datasets.
4. Measure a dictionary over the WKB, then native encoding once SedonaDB reads it.
