# Pipeline

How an observation reaches the store, and what is derived from it. The store's own layout,
formats and rules are in [medallion.md](medallion.md); this is what fills it.

## Capture

A phone runs the page the `server` crate serves from fly.io, samples GPS and accelerometer,
and sends timestamped JSON over a websocket. The server `LPUSH`es each sample onto an
Upstash redis list. Redis is optional: unset, the server logs samples rather than queueing
them, which is how it runs locally.

The queue is a landing format, not an archive. `recorder` drains it into the bronze
telemetry datasets — the verbatim payload alongside the readings interpreted from it — and
draining is destructive, so what has not been drained is the only copy.

The other two bronze writers pull rather than receive. `motis_poll` queries a local Motis
server for trains near recently logged positions and appends each poll to a capture log; see
[motis.md](motis.md). `extract` takes point-in-time Overture extracts of a country's rail,
water, and administrative divisions.

## Derivation

Silver is derived from bronze, and gold from silver. Each derivation replaces what it
produces, so any of them can be re-run over unchanged input to the same result.

```
bronze telemetry    ──sessionise──────▶ session, session_sample
bronze motis log    ──motis_ingest────▶ train_segment
bronze overture     ──notebook────────▶ water_crossing
session + crossings ──match_crossings─▶ session_crossing
water_crossing      ──pack_crossings──▶ gold crossings.pointset
```

Two properties of that graph matter more than the order:

- **An Overture extract is a prerequisite for the observation derivations, not only for the
  crossings.** A session and a train leg are each placed in a country, and the country
  decides the projected CRS their geometry is written in. The country areas come from the
  newest extract, so a store without one cannot derive silver at all.
- **The crossings half is the slow half.** Intersecting a country's rail against its water is
  the longest step in a rebuild, and its result changes only when the extract or the collapse
  tuning does. Re-deriving sessions after a drain does not require re-deriving it.

## Languages

**Rust derives; Python reads.** Every derivation that writes the store is Rust, or is a
notebook writing through the Rust implementation — there is one implementation of the silver
format and no second one to keep in step. Python reads: `visualise` converts the store to a
rerun recording with DuckDB, and notebooks explore it.

The one exception proves the rule. The water crossings derivation stays a marimo notebook,
because the work is spatial SQL and iteration on it is visual, but its write goes through
the `lookout_medallion` extension module rather than through a python parquet writer.

## Consumers

`visualise` produces a rerun `.rrd` from the bronze sensor datasets and the silver train
legs. The M5 device holds the gold point buffer in flash and scans it against each GPS fix;
see [device.md](device.md).

## Secrets

The redis URL is the only secret. Checked-in `deploy/*.env` files hold `op://` references
rather than values; local runs wrap the binary in `op run`, and deploys push the resolved
value with `fly secrets set`.
