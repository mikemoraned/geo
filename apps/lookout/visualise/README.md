# visualise

Converts a lookout SQLite archive (produced by the `recorder` cli) into a rerun
`.rrd` for visualisation. It reads the derived per-sensor tables (`accel`, `gps`),
selecting by a relative time window and optionally by device, and logs them under
per-device entity paths with a blueprint: a map view for each device's gps track and
a time-series view for its accel axes.

If the archive has also been enriched (a `transport` table, produced by `just enrich`),
the Overture rail network is logged as static geometry under `/transport` — segments as
`GeoLineStrings` coloured by rail class, connectors as `GeoPoints` — with its own shared
map pane alongside the device tiles.

## Usage

Run from `apps/lookout` (so the default `data/…` paths resolve):

```sh
just visualise --since 7d                          # all devices, last 7 days
just visualise --since 12h --devices 77a cdf2      # selected devices (id prefixes)
just visualise --since 7d --open                   # also push to a running viewer
```

`--since` is required (`30m`, `12h`, `7d`, `2w`). `--devices` takes id *prefixes*, so a
few leading characters are enough. `--near <degrees>` restricts the rail segments drawn
to those within that distance of a gps fix — a raw lon/lat **degrees** cut (a rough hack,
not true ground distance), off by default. Defaults: input `data/lookout.sqlite`, output
`data/lookout.rrd`. Open the result with the rerun viewer:

```sh
rerun data/lookout.rrd
```

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
