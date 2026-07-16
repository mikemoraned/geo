# visualise

Converts a lookout SQLite archive (produced by the `recorder` cli) into a rerun
`.rrd` for visualisation. It reads the derived per-sensor tables (`accel`, `gps`),
selecting by a relative time window and optionally by device, and logs them under
per-device entity paths with a blueprint: a map view for each device's gps track and
a time-series view for its accel axes.

## Usage

Run from `apps/lookout` (so the default `data/…` paths resolve):

```sh
just visualise --since 7d                          # all devices, last 7 days
just visualise --since 12h --devices <uuid> <uuid> # selected devices
```

`--since` is required (`30m`, `12h`, `7d`, `2w`). Defaults: input `data/lookout.sqlite`,
output `data/lookout.rrd`. Open the result with the rerun viewer:

```sh
rerun data/lookout.rrd
```
