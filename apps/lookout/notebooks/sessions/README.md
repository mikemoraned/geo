# sessions

Looking at the silver `session` and `session_sample` datasets `just silver-sessionise` derives.

`v1.py` reads them with DuckDB (spatial, so a GeoParquet geometry column comes back as
geometry), turns them into GeoDataFrames in lat/lon, and draws them with
`GeoDataFrame.explore`: every session path of a chosen day, one colour per session, and the
samples of a selected session as the accuracy circle each one claims — buffered on the
projected geometry, so the radius is metres to scale.

Run it with `just marimo sessions v1` from `notebooks/`.
