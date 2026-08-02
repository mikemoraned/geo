# visualise

Converts the lookout medallion store into a rerun `.rrd` for visualisation. It reads the
bronze sensor datasets (`gps_reading`, `accel_reading`, written by the `recorder` cli),
selecting by a relative time window and optionally by device, and logs them under
per-device entity paths with a blueprint: a map view for each device's gps track and a
time-series view for its accel aggregates.

Where the silver `train_segment` dataset has been derived (by `just silver-motis-ingest`), each
train is also logged as a moving dot interpolated along its route, under
`/trains/{label}/{trip_id}`, plus a shared overview map showing the trains and the gps
traces together.

Reading is done with DuckDB, the engine already used for notebook and ad-hoc work against
this store. Its spatial extension reads the silver GeoParquet geometry as geometry, so the
route coordinates and the interpolated positions come out of the query rather than being
decoded here — `INSTALL spatial` runs on startup, which needs network access the first time.

## Usage

Run from `apps/lookout` (so the default `data/…` paths resolve):

```sh
just visualise --since 7d                          # all devices, last 7 days
just visualise --since 12h --devices 77a cdf2      # selected devices (id prefixes)
just visualise --since 7d --open                   # also push to a running viewer
```

`--since` is required (`30m`, `12h`, `7d`, `2w`). `--devices` takes id *prefixes*, so a
few leading characters are enough. Defaults: input `--medallion-root
~/Data/geo/lookout/medallion`, output `data/lookout.rrd`. Open the result with the rerun
viewer:

```sh
rerun data/lookout.rrd
```

The Overture rail network pane is not currently drawn: it was fed by an `enrich` step that
has been removed, and returns when the silver rail derivation exists.

## `--open` and the persisted blueprint

The rerun viewer persists a layout (blueprint) per application id and restores it on
launch, so a layout from an earlier run can shadow later ones — e.g. after narrowing
`--devices`, you may see empty view panels for devices no longer in the data. This
survives restarting the viewer, because it's the *layout* that's persisted, not the
data.

`--open` avoids this: it tees the recording to both the `.rrd` file and a **running**
viewer (`127.0.0.1:9876`), pushing this run's blueprint as the *active* one so it
overrides whatever the viewer had persisted. Keep a viewer open and re-run with
`--open` to refresh it live. Without a running viewer it simply writes the file.

To clear a stale layout when opening a file by hand instead, run `rerun reset` once.
