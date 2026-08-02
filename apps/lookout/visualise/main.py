"""Convert the lookout medallion store into a rerun `.rrd` for visualisation.

Reads the bronze sensor datasets (`gps_reading`, `accel_reading`), selected by a relative
time window (`--since 7d`) and optionally by device (`--devices <uuid> ...`), and logs them
to a rerun recording under per-device entity paths. A blueprint gives each selected device
a map view for its speed-coloured gps track and a time-series view of its accel
ride-quality aggregates (rms roughness, peak jolts).

Where the silver `train_segment` dataset has been derived, each nearby train is also logged
as a moving dot under `/trains/{trip_id}` — interpolated along its route by its
realtime-corrected times, coloured by `mode`/`routeColor` and labelled by train number —
and a shared overview map shows the trains moving alongside the gps traces over the same
window.

Everything is read with DuckDB, the engine already used for ad-hoc and notebook work
against this store: the store is parquet, and its silver geometry is GeoParquet, which
DuckDB's spatial extension reads as geometry rather than as a blob to decode here.

Run from `apps/lookout` via `just visualise` so the default paths resolve.
"""

import argparse
import datetime as dt
import re
import time
from pathlib import Path

import duckdb
import rerun as rr
import rerun.blueprint as rrb

TIMELINE = "time"


def _medallion_root_in_repo() -> Path:
    """The store in the repo, found by walking up for the workspace the way the Rust CLIs do.

    The walk starts at the working directory itself, which is where the workspace manifest
    sits when a recipe is run as documented, and ends at the filesystem root.
    """
    start = Path.cwd().resolve()
    for directory in (start, *start.parents):
        manifest = directory / "Cargo.toml"
        if manifest.is_file() and "[workspace]" in manifest.read_text():
            return directory / "data/medallion"
    raise FileNotFoundError(
        f"no Cargo.toml declaring [workspace] at or above {start}, so the store's location "
        f"cannot be worked out; pass --medallion-root to say where it is"
    )


DEFAULT_MEDALLION_ROOT = _medallion_root_in_repo()
DEFAULT_OUTPUT = Path("data/lookout.rrd")

BRONZE = "bronze"
SILVER = "silver"

_DURATION_UNITS = {"s": 1, "m": 60, "h": 3600, "d": 86400, "w": 604800}


def parse_since(text: str) -> int:
    """Parse a `<number><unit>` relative duration (e.g. ``7d``) into seconds."""
    match = re.fullmatch(r"(\d+)([smhdw])", text.strip())
    if not match:
        raise argparse.ArgumentTypeError(
            f"invalid --since {text!r}; expected e.g. 30m, 12h, 7d"
        )
    value, unit = match.groups()
    return int(value) * _DURATION_UNITS[unit]


class Store:
    """A DuckDB session over one medallion store.

    A dataset is named by its layer and name — the layout `docs/medallion.md` pins — and
    read with hive partitioning on, so its partition keys come back as typed columns that a
    predicate can prune whole files by. Queries write `{dataset}` where they select from
    one, and bind their values as named parameters.
    """

    def __init__(self, root: Path) -> None:
        self.root = root
        self.con = duckdb.connect()
        # Spatial reads a GeoParquet geometry column as geometry rather than as a WKB blob,
        # and provides the along-the-line interpolation the train dots are sampled with.
        self.con.execute("INSTALL spatial; LOAD spatial;")

    def rows(self, layer: str, dataset: str, sql: str, **params) -> list[tuple]:
        """Rows of `sql` against `dataset`, which `{dataset}` in the query stands for.

        A dataset that has never been written selects nothing rather than failing, so a
        store whose trains have never been derived reads as a store with no trains.
        """
        directory = self.root / layer / dataset
        if not directory.is_dir():
            return []
        source = "read_parquet($dataset, hive_partitioning = 1)"
        return self.con.execute(
            sql.format(dataset=source),
            {"dataset": str(directory / "**" / "*.parquet"), **params},
        ).fetchall()


# Restricts to the given device id prefixes, or to all devices when they are NULL. Each
# prefix matches any device id it is a prefix of, so a few leading characters of a uuid are
# enough to select it.
_DEVICES = """
    ($devices IS NULL
     OR len(list_filter($devices, prefix -> starts_with(device_id, prefix))) > 0)
"""

# The window over the sensor datasets: readings taken at or after the cutoff.
#
# The second predicate is over `ingested_date`, the partition key, so it prunes whole files
# rather than filtering their rows — DuckDB types an inferred hive key as `DATE` and reports
# `Scanning Files: n/m` for a predicate over it. It is sound because a reading is ingested
# after it is taken, so nothing inside the window sits in a partition older than the
# window. The exception is a device whose clock runs ahead of real time by more than the
# window, whose readings are mistimed however they are selected.
_READING_WINDOW = f"""
    t >= to_timestamp($cutoff_s)
    AND ingested_date >= $cutoff_date
    AND {_DEVICES}
"""

# `n = 0` says no readings were aggregated into the window, so its `rms`/`peak` describe
# nothing — either the sampling was suspended, or the reading predates the aggregates being
# captured at all and they are zero by default. Plotted, those rows read as a real flat zero,
# so they are dropped instead. The cost is that a suspended window no longer shows in the
# capture-health series, where its zero would have been informative.
_ACCEL = f"""
    SELECT device_id, epoch_ms(t) AS t, rms, peak, n
    FROM {{dataset}}
    WHERE {_READING_WINDOW}
      AND n > 0
    ORDER BY t
"""

_GPS = f"""
    SELECT device_id, epoch_ms(t) AS t, lat, lon, acc, speed
    FROM {{dataset}}
    WHERE {_READING_WINDOW}
    ORDER BY t
"""


def _windowed(
    store: Store, layer: str, dataset: str, sql: str, cutoff_ms: int, **params
) -> list[tuple]:
    """Run a windowed query, binding the cutoff both ways it is used: as the instant rows are
    compared against, and as the date partitions are pruned by."""
    cutoff = dt.datetime.fromtimestamp(cutoff_ms / 1000.0, dt.timezone.utc)
    return store.rows(
        layer,
        dataset,
        sql,
        cutoff_s=cutoff.timestamp(),
        cutoff_date=cutoff.date(),
        **params,
    )


def fetch_accel(store: Store, cutoff_ms: int, devices: list[str] | None):
    """Rows `(device_id, t, rms, peak, n)` of the bronze `accel_reading` dataset."""
    return _windowed(
        store, BRONZE, "accel_reading", _ACCEL, cutoff_ms, devices=devices or None
    )


def fetch_gps(store: Store, cutoff_ms: int, devices: list[str] | None):
    """Rows `(device_id, t, lat, lon, acc, speed)` of the bronze `gps_reading` dataset."""
    return _windowed(
        store, BRONZE, "gps_reading", _GPS, cutoff_ms, devices=devices or None
    )


def log_accel(rows) -> None:
    """Log accel aggregates per device: ride quality under `device/{id}/accel` (`rms`
    roughness and `peak` jolts / pointwork) and capture health under
    `device/{id}/samples` (`n`, the readings-per-window).

    `rms`/`peak` are the accel signals that mean something at the 0.1 Hz sample rate; the
    raw instantaneous `x,y,z` (kept in the dataset as a tilt view) aliases into noise and is
    not plotted. `n` sits on its own path because at ~600 it would dwarf them — it should
    hold near-constant while sampling, so it reads as a capture-health check rather than a
    ride signal. Readings that aggregated nothing are already excluded by the query.

    Written column-wise with `rr.send_columns` (the natural shape for a table→rrd
    converter); `Scalars` needs the same count at every timestamp.
    """
    by_device: dict[str, list[tuple[float, float, float, int]]] = {}
    for device_id, t, rms, peak, n in rows:
        by_device.setdefault(device_id, []).append((t / 1000.0, rms, peak, n))

    for device_id, samples in by_device.items():
        index = [rr.TimeColumn(TIMELINE, timestamp=[s[0] for s in samples])]

        accel_path = f"device/{device_id}/accel"
        # Static legend labels — without them the series are unnamed.
        rr.log(accel_path, rr.SeriesLines(names=["rms", "peak"]), static=True)
        values = [v for (_, rms, peak, _) in samples for v in (rms, peak)]
        rr.send_columns(
            accel_path,
            indexes=index,
            columns=rr.Scalars.columns(scalars=values).partition([2] * len(samples)),
        )

        samples_path = f"device/{device_id}/samples"
        rr.log(samples_path, rr.SeriesLines(names=["n"]), static=True)
        rr.send_columns(
            samples_path,
            indexes=index,
            columns=rr.Scalars.columns(scalars=[s[3] for s in samples]),
        )


# Viridis anchor stops (perceptually uniform, colour-blind friendly), interpolated in
# RGB for the speed-coloured track. Speed with no fix (stationary / unknown) is grey.
_VIRIDIS = [
    (0.0, (68, 1, 84)),
    (0.25, (59, 82, 139)),
    (0.5, (33, 145, 140)),
    (0.75, (94, 201, 98)),
    (1.0, (253, 231, 37)),
]
_NO_SPEED = (128, 128, 128)


def _viridis(t: float) -> tuple[int, int, int]:
    """Map ``t`` in [0, 1] to an RGB triple along the viridis ramp."""
    t = max(0.0, min(1.0, t))
    for (t0, c0), (t1, c1) in zip(_VIRIDIS, _VIRIDIS[1:]):
        if t <= t1:
            f = 0.0 if t1 == t0 else (t - t0) / (t1 - t0)
            return tuple(round(a + (b - a) * f) for a, b in zip(c0, c1))
    return _VIRIDIS[-1][1]


def _speed_color(speed, lo: float, hi: float) -> tuple[int, int, int]:
    """Colour for a Doppler `speed` (m/s), scaled over the track's own [lo, hi]."""
    if speed is None:
        return _NO_SPEED
    norm = 0.5 if hi <= lo else (speed - lo) / (hi - lo)
    return _viridis(norm)


def log_gps(rows) -> None:
    """Log gps rows as per-timestamp points under `device/{id}/gps` plus one static
    per-device track under `device/{id}/track`, coloured by Doppler speed.

    The per-timestamp `GeoPoints` each overwrite the last (a cursor dot that follows
    the timeline), so on their own the map shows a single dot jumping between fixes.
    The static track draws the whole journey as always-visible `GeoLineStrings`, split
    into one segment per fix so each can be coloured by the speed at its start.
    """
    tracks: dict[str, list[tuple[float, float, float | None]]] = {}
    for device_id, t, lat, lon, acc, speed in rows:
        rr.set_time(TIMELINE, timestamp=t / 1000.0)
        # `acc` is the reported horizontal accuracy in metres; map scene units are
        # metres too, so it draws as a true-scale uncertainty circle. Omit the radius
        # when accuracy is unknown.
        radii = [acc] if acc is not None else None
        rr.log(f"device/{device_id}/gps", rr.GeoPoints(lat_lon=[(lat, lon)], radii=radii))
        tracks.setdefault(device_id, []).append((lat, lon, speed))

    for device_id, points in tracks.items():
        _log_track(device_id, points)


def _log_track(device_id: str, points: list[tuple[float, float, float | None]]) -> None:
    path = f"device/{device_id}/track"
    if len(points) < 2:
        # A single fix has no segment to colour — draw the bare point-run.
        line = [(lat, lon) for lat, lon, _ in points]
        rr.log(path, rr.GeoLineStrings(lat_lon=[line]), static=True)
        return

    speeds = [s for _, _, s in points if s is not None]
    lo = min(speeds) if speeds else 0.0
    hi = max(speeds) if speeds else 0.0
    segments = [
        [(a[0], a[1]), (b[0], b[1])] for a, b in zip(points, points[1:])
    ]
    colors = [_speed_color(a[2], lo, hi) for a in points[:-1]]
    rr.log(path, rr.GeoLineStrings(lat_lon=segments, colors=colors), static=True)


# Categorical colours per transit `mode`. DELFI reports correct `route_type`, so `mode`
# already separates long-distance (HIGHSPEED_RAIL/LONG_DISTANCE) from regional rail — a
# train's GTFS `routeColor` still wins when the feed carries one. Unmapped → grey.
_MODE_COLORS = {
    "HIGHSPEED_RAIL": (214, 39, 40),
    "LONG_DISTANCE": (214, 39, 40),
    "NIGHT_RAIL": (140, 86, 75),
    "REGIONAL_FAST_RAIL": (255, 127, 14),
    "REGIONAL_RAIL": (31, 119, 180),
    "RAIL": (31, 119, 180),
    "METRO": (148, 103, 189),
    "SUBWAY": (148, 103, 189),
    "TRAM": (255, 127, 14),
    "BUS": (44, 160, 44),
    "FERRY": (23, 190, 207),
}
_MODE_DEFAULT = (127, 127, 127)

# How often (seconds) a train's interpolated position is sampled along a leg. Rerun holds
# a `GeoPoints` value until the next one, so the dot is resampled to animate along the
# line rather than jumping stop-to-stop.
SAMPLE_STEP_S = 10

# The window over the derived legs: those still active at or after the cutoff, so the trains
# cover the same window as the gps traces. The `departure_date` predicate prunes partitions
# (see `_READING_WINDOW`) and allows a day's slack, since a leg departs before it arrives.
_LEG_WINDOW = """
    SELECT trip_id, mode, route_color, route_name, train_number, geometry,
           epoch_ms(departure) AS departure_ms,
           greatest(epoch_ms(arrival) - epoch_ms(departure), 0) AS span_ms
    FROM {dataset}
    WHERE arrival >= to_timestamp($cutoff_s)
      AND departure_date >= $cutoff_date - INTERVAL 1 DAY
"""

# One row per leg with its route as a list of rerun `[lat, lon]` vertices. Silver geometry
# is lon/lat (x-y) order, as simple features require, so each vertex is read out flipped.
# Vertices are taken with the ordinary accessors rather than by casting to DuckDB's native
# `LINESTRING_2D`, which the engine refuses for a geometry column whose CRS it recognised.
_TRAIN_LEGS = f"""
    WITH leg AS ({_LEG_WINDOW})
    SELECT trip_id, mode, route_color, route_name, train_number,
           list_transform(
               generate_series(1, ST_NPoints(geometry)),
               i -> [ST_Y(ST_PointN(geometry, i::INTEGER)),
                     ST_X(ST_PointN(geometry, i::INTEGER))]
           ) AS route
    FROM leg
    ORDER BY trip_id, departure_ms
"""

# The moving dot: each leg's position resampled at `$step_ms` along its span, plus both
# endpoints, placed by length-normalised interpolation along the route. A leg of zero or
# negative span yields only its start point.
_TRAIN_POSITIONS = f"""
    WITH leg AS ({_LEG_WINDOW}),
         sample AS (
           SELECT trip_id, geometry, departure_ms, span_ms,
                  unnest(list_distinct(
                      list_append(generate_series(0, span_ms, $step_ms), span_ms)
                  )) AS offset_ms
           FROM leg
         ),
         located AS (
           SELECT trip_id, departure_ms + offset_ms AS t,
                  ST_LineInterpolatePoint(
                      geometry,
                      CASE WHEN span_ms = 0 THEN 0.0 ELSE offset_ms::DOUBLE / span_ms END
                  ) AS position
           FROM sample
         )
    SELECT trip_id, t, ST_Y(position) AS lat, ST_X(position) AS lon
    FROM located
    ORDER BY trip_id, t
"""


def fetch_train_legs(store: Store, cutoff_ms: int):
    """Rows `(trip_id, mode, route_color, route_name, train_number, route)` of the silver
    `train_segment` dataset, `route` being the leg's vertices as `[lat, lon]`."""
    return _windowed(store, SILVER, "train_segment", _TRAIN_LEGS, cutoff_ms)


def fetch_train_positions(store: Store, cutoff_ms: int, step_s: int = SAMPLE_STEP_S):
    """Rows `(trip_id, t, lat, lon)` sampling each leg's interpolated position every
    `step_s` seconds."""
    return _windowed(
        store,
        SILVER,
        "train_segment",
        _TRAIN_POSITIONS,
        cutoff_ms,
        step_ms=step_s * 1000,
    )


def _hex_rgb(text: str) -> tuple[int, int, int] | None:
    """Parse an `RRGGBB` (or `#RRGGBB`) hex colour to an `(r, g, b)` triple, or None."""
    s = text.lstrip("#")
    if len(s) != 6:
        return None
    try:
        return tuple(int(s[i : i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        return None


def _train_color(mode: str, route_color: str | None) -> tuple[int, int, int]:
    """Colour for a train: its GTFS `routeColor` (hex) when present and parseable, else a
    per-`mode` colour (DELFI's `mode` separates long-distance from regional), else grey."""
    if route_color:
        rgb = _hex_rgb(route_color)
        if rgb is not None:
            return rgb
    return _MODE_COLORS.get(mode, _MODE_DEFAULT)


def _train_label(train_number: int | None, route_name: str | None) -> str | None:
    """The train's human label: the train number (e.g. `2569`) when known, else the line
    (`route_name`, e.g. `RE4`), else nothing. The number is the identifier a passenger
    matches to a ticket/platform; `mode`/colour carries the product family."""
    if train_number is not None:
        return str(train_number)
    return route_name or None


def _train_entity(trip_id: str, label: str | None) -> str:
    """The rerun entity path for a train. `GeoPoints` can't render an on-map text label
    (rerun 0.34), so the human label is surfaced in the path instead — visible on hover
    and grouped in the streams tree — while `trip_id` stays the unique leaf so distinct
    trips sharing a label (or a `route_name` fallback) never collide."""
    if label:
        slug = label.replace("/", "-").replace(" ", "")
        return f"trains/{slug}/{trip_id}"
    return f"trains/{trip_id}"


def _trains(legs) -> dict:
    """Group leg rows per trip: `{trip_id: {"entity", "color", "route": [[[lat, lon]…]…]}}`.

    A trip's colour and label come from its first leg — they identify the train, not the
    leg — and `route` is one lat/lon polyline per leg.
    """
    trains: dict = {}
    for trip_id, mode, route_color, route_name, train_number, route in legs:
        entry = trains.setdefault(
            trip_id,
            {
                "entity": _train_entity(trip_id, _train_label(train_number, route_name)),
                "color": _train_color(mode, route_color),
                "route": [],
            },
        )
        entry["route"].append(route)
    return trains


def log_trains(legs, positions) -> None:
    """Log each trip as a moving `GeoPoints` dot that follows the timeline, plus its static
    route line(s) under `{entity}/route`, coloured by `routeColor`/`mode`. The entity path
    carries the train number (falling back to the line) so the dot is identifiable on hover
    and in the streams tree.

    The dot uses the same per-timestamp `GeoPoints` idiom as the gps cursor, so it moves
    across the shared map over the same window as the gps traces.
    """
    trains = _trains(legs)
    for entry in trains.values():
        rr.log(
            f"{entry['entity']}/route",
            rr.GeoLineStrings(
                lat_lon=entry["route"], colors=[entry["color"]] * len(entry["route"])
            ),
            static=True,
        )

    for trip_id, t, lat, lon in positions:
        entry = trains[trip_id]
        rr.set_time(TIMELINE, timestamp=t / 1000.0)
        rr.log(
            entry["entity"],
            rr.GeoPoints(lat_lon=[(lat, lon)], colors=[entry["color"]]),
        )


def build_blueprint(devices: list[str], has_trains: bool = False) -> rrb.Blueprint:
    """Per device: a static full-route map, a latest-position map that follows the
    timeline, a ride-quality time-series view (rms/peak), and a capture-health view
    (n) — tiled in a grid. When the store has train data, a shared overview map (root
    origin) shows the gps traces and the moving trains together, so they share one view
    over the same timeline window.

    The `track` map shows the whole journey as a static speed-coloured polyline; the
    `gps` map shows only the per-timestamp point, which moves as the timeline cursor
    advances.
    """
    panes = [
        rrb.Vertical(
            rrb.Horizontal(
                rrb.MapView(
                    origin=f"/device/{device_id}/track", name=f"{device_id} route"
                ),
                rrb.MapView(
                    origin=f"/device/{device_id}/gps", name=f"{device_id} latest"
                ),
            ),
            rrb.TimeSeriesView(
                origin=f"/device/{device_id}/accel", name=f"{device_id} ride"
            ),
            rrb.TimeSeriesView(
                origin=f"/device/{device_id}/samples", name=f"{device_id} capture"
            ),
        )
        for device_id in devices
    ]
    if has_trains:
        # A root-origin map so the gps device tracks and the moving trains render in one
        # shared view, over the recording's single (shared) timeline window.
        panes.append(rrb.MapView(origin="/", name="gps + trains"))
    return rrb.Blueprint(rrb.Grid(*panes), collapse_panels=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--since",
        type=parse_since,
        required=True,
        metavar="DURATION",
        help="how far back to select, e.g. 30m, 12h, 7d",
    )
    parser.add_argument(
        "--devices",
        nargs="*",
        metavar="PREFIX",
        help="device id prefixes to include (default: all devices in the window)",
    )
    parser.add_argument(
        "--medallion-root",
        type=Path,
        default=DEFAULT_MEDALLION_ROOT,
        help="root of the medallion data store",
    )
    parser.add_argument(
        "--output", type=Path, default=DEFAULT_OUTPUT, help="output .rrd recording"
    )
    parser.add_argument(
        "--open",
        action="store_true",
        help="also send to a running rerun viewer, forcing this run's layout to be "
        "active (overrides any blueprint the viewer has persisted for this app)",
    )
    args = parser.parse_args()

    cutoff_ms = int((time.time() - args.since) * 1000)

    store = Store(args.medallion_root)
    accel = fetch_accel(store, cutoff_ms, args.devices)
    gps = fetch_gps(store, cutoff_ms, args.devices)

    devices = sorted({row[0] for row in accel} | {row[0] for row in gps})
    if not devices:
        raise SystemExit(
            f"no samples in the selected window (since={args.since}s, "
            f"devices={args.devices or 'all'})"
        )

    legs = fetch_train_legs(store, cutoff_ms)
    positions = fetch_train_positions(store, cutoff_ms)

    rr.init("lookout")
    log_accel(accel)
    log_gps(gps)
    if legs:
        log_trains(legs, positions)
    blueprint = build_blueprint(devices, has_trains=bool(legs))

    if args.open:
        # Tee to the file and a running viewer. The viewer persists a blueprint per
        # application id and would otherwise keep showing a stale layout, so push
        # this run's blueprint as the *active* one to override it.
        rr.set_sinks(rr.FileSink(path=str(args.output)), rr.GrpcSink())
        rr.send_blueprint(blueprint, make_active=True, make_default=True)
        where = f"{args.output} (+ running viewer)"
    else:
        rr.save(args.output, default_blueprint=blueprint)
        where = str(args.output)

    print(
        f"wrote {where}: {len(accel)} accel + {len(gps)} gps samples "
        f"+ {len(legs)} train segments across {len(devices)} device(s)"
    )


if __name__ == "__main__":
    main()
