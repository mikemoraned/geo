# lookout architecture

Data pipeline (`apps/lookout`), as it stands today: phone → fly.io `server` → Upstash redis
list (`lookout-telemetry`) → `recorder drain` writes the bronze telemetry datasets in the
medallion store defined in `apps/lookout/docs/medallion.md`.

- `telemetry` crate owns the redis contract: `latest_samples` (non-destructive LRANGE),
  `brpop_sample`/`drain` (destructive).
- `motis_poll` writes bronze `motis_segment`; `motis_ingest` derives silver `train_segment`.
- `transport`'s `extract` bin takes point-in-time Overture extracts into bronze.
- `visualise/` (Python uv + rerun) reads the store with DuckDB → `.rrd`.

The sqlite dbs under `data/` are now history awaiting backfill, not part of the pipeline;
nothing reads or writes them. Under the medallion scheme redis and sqlite are
landing/external only, and everything from bronze onwards is parquet, with silver as
GeoParquet readable by any engine in use.

**Convention that guides new work:** Rust derives; Python `visualise` only reads. Rust
writes each derivation into the medallion store rather than adding a table to the sqlite,
and new geospatial derivations follow the silver rules (GeoParquet, WKB geometry, lat/lon
in CRS 84) rather than the older `transport` table shape. Redis URL comes from
`LOOKOUT_REDIS_URL` via `op run` (1Password) in Justfile recipes.

See [motis-trips-api](motis-trips-api.md).
