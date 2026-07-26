# lookout architecture

Data pipeline (`apps/lookout`), as it stands today: phone → fly.io `server` → Upstash redis list
(`lookout-telemetry`) → `recorder` cli drains into `data/lookout.sqlite` — a lossless
`raw(md5,json)` table + deduped per-sensor `gps`/`accel` tables, all `INSERT OR IGNORE`.

- `telemetry` crate owns the redis contract: `latest_samples` (non-destructive LRANGE),
  `brpop_sample`/`drain` (destructive).
- `transport` crate's `enrich` bin derives per-(device,day) bboxes and writes an Overture
  rail `transport` table (WKB geom + R*Tree) into the same sqlite.
- `visualise/` (Python uv + rerun) reads the sqlite → `.rrd`.

**This sqlite-centred layout is being migrated** to the layered store defined in
`apps/lookout/docs/medallion.md`. Under that scheme redis and the sqlite dbs are
landing/external only — a live capture format drained into bronze — and everything from
bronze onwards is parquet, with silver as GeoParquet readable by any engine in use. Delete
the description above once the migration lands — this note records the current pipeline,
not its history.

**Convention that guides new work:** Rust derives; Python `visualise` only reads. Rust
writes each derivation into the medallion store rather than adding a table to the sqlite,
and new geospatial derivations follow the silver rules (GeoParquet, WKB geometry, lat/lon
in CRS 84) rather than the older `transport` table shape. Redis URL comes from
`LOOKOUT_REDIS_URL` via `op run` (1Password) in Justfile recipes.

See [motis-trips-api](motis-trips-api.md).
