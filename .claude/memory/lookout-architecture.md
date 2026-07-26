# lookout architecture

Data pipeline (`apps/lookout`): phone → fly.io `server` → Upstash redis list
(`lookout-telemetry`) → `recorder` cli drains into `data/lookout.sqlite` — a lossless
`raw(md5,json)` table + deduped per-sensor `gps`/`accel` tables, all `INSERT OR IGNORE`.

- `telemetry` crate owns the redis contract: `latest_samples` (non-destructive LRANGE),
  `brpop_sample`/`drain` (destructive).
- `transport` crate's `enrich` bin derives per-(device,day) bboxes and writes an Overture
  rail `transport` table (WKB geom + R*Tree) into the same sqlite.
- `visualise/` (Python uv + rerun) reads the sqlite → `.rrd`.

**Convention that guides new work:** Rust writes derived tables into the sqlite; Python
`visualise` only reads. New geospatial derivations follow the `transport` table shape
(WKB geometry read back with shapely). Redis URL comes from `LOOKOUT_REDIS_URL` via
`op run` (1Password) in Justfile recipes.

See [motis-trips-api](motis-trips-api.md).
