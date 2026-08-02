# visualise

Converts the lookout medallion store into a rerun `.rrd` for visualisation. `main.py`'s
docstring describes what it reads and logs, and is also its `--help`.

## Usage

Run from `apps/lookout`, so the default paths resolve:

```sh
just visualise --since 7d                          # all devices, last 7 days
just visualise --since 12h --devices 77a cdf2      # selected devices (id prefixes)
just visualise --since 7d --open                   # also push to a running viewer
```

Open the result with the rerun viewer:

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
