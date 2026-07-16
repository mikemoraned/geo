"""Convert a lookout SQLite archive into a rerun `.rrd` for visualisation.

Reads the derived per-sensor tables (`accel`, `gps`), selected by a relative time
window (`--since 7d`) and optionally by device (`--devices <uuid> ...`), and logs
them to a rerun recording under per-device entity paths. A blueprint gives each
selected device a map view for its gps track and a time-series view for its accel
axes.

Run from `apps/lookout` via `just visualise` so the default paths resolve.
"""

import argparse
import re
import sqlite3
import time
from pathlib import Path

import rerun as rr
import rerun.blueprint as rrb

TIMELINE = "time"
DEFAULT_DB = Path("data/lookout.sqlite")
DEFAULT_OUTPUT = Path("data/lookout.rrd")

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


def _device_filter(devices: list[str] | None) -> tuple[str, list[str]]:
    """SQL fragment + params restricting to `devices`, or all devices if None.

    Each entry matches any device id it is a *prefix* of, so a few leading
    characters of a uuid are enough to select it.
    """
    if not devices:
        return "", []
    terms = " OR ".join("substr(device_id, 1, length(?)) = ?" for _ in devices)
    params = [p for prefix in devices for p in (prefix, prefix)]
    return f" AND ({terms})", params


def fetch_accel(conn: sqlite3.Connection, cutoff_ms: int, devices: list[str] | None):
    clause, params = _device_filter(devices)
    return conn.execute(
        f"SELECT device_id, t, x, y, z FROM accel WHERE t >= ?{clause} ORDER BY t",
        [cutoff_ms, *params],
    ).fetchall()


def fetch_gps(conn: sqlite3.Connection, cutoff_ms: int, devices: list[str] | None):
    clause, params = _device_filter(devices)
    return conn.execute(
        f"SELECT device_id, t, lat, lon, acc FROM gps WHERE t >= ?{clause} ORDER BY t",
        [cutoff_ms, *params],
    ).fetchall()


def log_accel(rows) -> None:
    """Log accel rows as per-axis scalar series under `device/{id}/accel/{x,y,z}`,
    plus an orientation-invariant magnitude `|a|` under `device/{id}/accel/magnitude`.

    The raw axes are gravity-dominated in an unknown device orientation, so the
    magnitude is the one accel signal that means something on its own — expect a flat
    ~9.81 with spikes where the phone was handled.
    """
    for device_id, t, x, y, z in rows:
        rr.set_time(TIMELINE, timestamp=t / 1000.0)
        for axis, value in (("x", x), ("y", y), ("z", z)):
            if value is not None:
                rr.log(f"device/{device_id}/accel/{axis}", rr.Scalars(value))
        if x is not None and y is not None and z is not None:
            magnitude = (x * x + y * y + z * z) ** 0.5
            rr.log(f"device/{device_id}/accel/magnitude", rr.Scalars(magnitude))


def log_gps(rows) -> None:
    """Log gps rows as per-timestamp points under `device/{id}/gps` plus one static
    polyline per device under `device/{id}/track`.

    The per-timestamp `GeoPoints` each overwrite the last (a cursor dot that follows
    the timeline), so on their own the map shows a single dot jumping between fixes.
    The static `GeoLineStrings` draws the whole journey as a path that is always
    visible.
    """
    tracks: dict[str, list[tuple[float, float]]] = {}
    for device_id, t, lat, lon, acc in rows:
        rr.set_time(TIMELINE, timestamp=t / 1000.0)
        # `acc` is the reported horizontal accuracy in metres; map scene units are
        # metres too, so it draws as a true-scale uncertainty circle. Omit the radius
        # when accuracy is unknown.
        radii = [acc] if acc is not None else None
        rr.log(f"device/{device_id}/gps", rr.GeoPoints(lat_lon=[(lat, lon)], radii=radii))
        tracks.setdefault(device_id, []).append((lat, lon))

    for device_id, path in tracks.items():
        rr.log(
            f"device/{device_id}/track",
            rr.GeoLineStrings(lat_lon=[path]),
            static=True,
        )


def build_blueprint(devices: list[str]) -> rrb.Blueprint:
    """Per device: a static full-route map, a latest-position map that follows the
    timeline, and an accel time-series view — tiled in a grid.

    The `track` map shows the whole journey as a static polyline; the `gps` map shows
    only the per-timestamp point, which moves as the timeline cursor advances.
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
                origin=f"/device/{device_id}/accel", name=f"{device_id} accel"
            ),
        )
        for device_id in devices
    ]
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
    parser.add_argument("--db", type=Path, default=DEFAULT_DB, help="input SQLite archive")
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

    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    try:
        accel = fetch_accel(conn, cutoff_ms, args.devices)
        gps = fetch_gps(conn, cutoff_ms, args.devices)
    finally:
        conn.close()

    devices = sorted({row[0] for row in accel} | {row[0] for row in gps})
    if not devices:
        raise SystemExit(
            f"no samples in the selected window (since={args.since}s, "
            f"devices={args.devices or 'all'})"
        )

    rr.init("lookout")
    log_accel(accel)
    log_gps(gps)
    blueprint = build_blueprint(devices)

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
        f"across {len(devices)} device(s)"
    )


if __name__ == "__main__":
    main()
