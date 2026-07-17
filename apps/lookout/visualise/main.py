"""Convert a lookout SQLite archive into a rerun `.rrd` for visualisation.

Reads the derived per-sensor tables (`accel`, `gps`), selected by a relative time
window (`--since 7d`) and optionally by device (`--devices <uuid> ...`), and logs
them to a rerun recording under per-device entity paths. A blueprint gives each
selected device a map view for its speed-coloured gps track and a time-series view of
its accel ride-quality aggregates (rms roughness, peak jolts).

If the archive has been enriched (a `transport` table, written by `enrich`), the
Overture rail network — segments coloured by class, plus their connectors — is logged
as static geometry under `/transport` and given its own shared map pane.

Run from `apps/lookout` via `just visualise` so the default paths resolve.
"""

import argparse
import re
import sqlite3
import time
from pathlib import Path

import rerun as rr
import rerun.blueprint as rrb
import shapely

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
        f"SELECT device_id, t, rms, peak, n, x, y, z FROM accel "
        f"WHERE t >= ?{clause} ORDER BY t",
        [cutoff_ms, *params],
    ).fetchall()


def fetch_gps(conn: sqlite3.Connection, cutoff_ms: int, devices: list[str] | None):
    clause, params = _device_filter(devices)
    return conn.execute(
        f"SELECT device_id, t, lat, lon, acc, speed, heading FROM gps "
        f"WHERE t >= ?{clause} ORDER BY t",
        [cutoff_ms, *params],
    ).fetchall()


def fetch_transport(conn: sqlite3.Connection):
    """Rows `(kind, class, geom)` from the `transport` table — the Overture rail
    segments and connectors written by `enrich`, with the geometry as a WKB blob.

    Not time-windowed: the transport network is static enrichment, logged whole. Empty
    when the archive has never been enriched, i.e. the table is absent.
    """
    exists = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'transport'"
    ).fetchone()
    if not exists:
        return []
    return conn.execute("SELECT kind, class, geom FROM transport").fetchall()


def log_accel(rows) -> None:
    """Log accel aggregates per device: ride quality under `device/{id}/accel` (`rms`
    roughness and `peak` jolts / pointwork) and capture health under
    `device/{id}/samples` (`n`, the readings-per-window).

    `rms`/`peak` are the accel signals that mean something at the 0.1 Hz sample rate;
    the raw instantaneous `x,y,z` (kept in the archive as a tilt view) aliases into
    noise and is not plotted. `n` sits on its own path because at ~600 it would dwarf
    them — it should hold near-constant while sampling and drop toward zero where the
    page was suspended, so it reads as a capture-health check rather than a ride
    signal. Rows without aggregates — legacy captures predating the columns — are
    dropped rather than shown as a misleading flat zero.

    Written column-wise with `rr.send_columns` (the natural shape for a table→rrd
    converter); `Scalars` needs the same count at every timestamp.
    """
    by_device: dict[str, list[tuple[float, float, float, int]]] = {}
    for device_id, t, rms, peak, n, *_ in rows:
        if rms is None or peak is None:
            continue
        by_device.setdefault(device_id, []).append((t / 1000.0, rms, peak, n or 0))

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
    for device_id, t, lat, lon, acc, speed, _heading in rows:
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


# Categorical colours for the rail `class`, so the map distinguishes gauges/modes.
# Anything unmapped (including the Overture `unknown` class) falls back to grey.
_CLASS_COLORS = {
    "standard_gauge": (31, 119, 180),
    "narrow_gauge": (44, 160, 44),
    "tram": (255, 127, 14),
    "subway": (148, 103, 189),
    "monorail": (214, 39, 40),
    "funicular": (140, 86, 75),
    "light_rail": (23, 190, 207),
}
_CLASS_DEFAULT = (127, 127, 127)
# Connectors are the shared junction nodes: one neutral dark dot for all of them.
_CONNECTOR_COLOR = (40, 40, 40)


def _class_color(rail_class: str | None) -> tuple[int, int, int]:
    """Colour for a rail `class`, or grey for an unknown/unmapped one."""
    return _CLASS_COLORS.get(rail_class, _CLASS_DEFAULT)


def _linestrings(shape) -> list:
    """The `LineString` parts of a segment geometry — one for a `LineString`, several
    for a `MultiLineString` — so each can be logged as its own polyline."""
    if shape.geom_type == "MultiLineString":
        return list(shape.geoms)
    return [shape]


def _transport_geometry(rows, gps_lonlat=None, near=None):
    """Transform `transport` rows into rerun-ready geometry, returning
    `(segments, segment_colors, connectors)`.

    The stored WKB is in `lon lat` (x-y) order, so every coordinate is flipped to
    rerun's `(lat, lon)`. Segments are coloured by rail `class`. When `near` is set, a
    segment is kept only if it comes within `near` of a gps fix — a raw **degrees**
    distance (see the `--near` help), not true ground distance.
    """
    near_window = (
        shapely.MultiPoint(gps_lonlat) if near is not None and gps_lonlat else None
    )
    segments: list[list[tuple[float, float]]] = []
    segment_colors: list[tuple[int, int, int]] = []
    connectors: list[tuple[float, float]] = []
    for kind, rail_class, geom in rows:
        shape = shapely.from_wkb(geom)
        if kind == "segment":
            for line in _linestrings(shape):
                if near_window is not None and line.distance(near_window) > near:
                    continue
                segments.append([(lat, lon) for lon, lat in line.coords])
                segment_colors.append(_class_color(rail_class))
        elif kind == "connector":
            connectors.append((shape.y, shape.x))
    return segments, segment_colors, connectors


def log_transport(rows, gps_lonlat=None, near=None) -> None:
    """Log the Overture transport network as static geometry: rail segments as
    `GeoLineStrings` under `transport/segments` coloured by rail `class`, and
    connectors as `GeoPoints` under `transport/connectors`.

    Static because the network is a fixed backdrop the device tracks move across, not
    something that changes over the recording's timeline.
    """
    segments, segment_colors, connectors = _transport_geometry(rows, gps_lonlat, near)
    if segments:
        rr.log(
            "transport/segments",
            rr.GeoLineStrings(lat_lon=segments, colors=segment_colors),
            static=True,
        )
    if connectors:
        rr.log(
            "transport/connectors",
            rr.GeoPoints(
                lat_lon=connectors, colors=[_CONNECTOR_COLOR] * len(connectors)
            ),
            static=True,
        )


def build_blueprint(devices: list[str], has_transport: bool) -> rrb.Blueprint:
    """Per device: a static full-route map, a latest-position map that follows the
    timeline, a ride-quality time-series view (rms/peak), and a capture-health view
    (n) — tiled in a grid. When the archive has transport data, one shared map of the
    Overture rail network is appended alongside.

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
    if has_transport:
        # The rail network (segments + connectors) is shared across devices, not
        # per-device, so it sits as its own map pane beside the device tiles.
        panes.append(rrb.MapView(origin="/transport", name="transport"))
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
        "--near",
        type=float,
        default=None,
        metavar="DEGREES",
        help="only show rail segments within this distance of a gps fix (default: show "
        "all). HACK: the distance is raw lon/lat degrees, not metres — a rough cut, "
        "not true ground distance (which would need reprojecting to a metric CRS).",
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
        transport = fetch_transport(conn)
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
    if transport:
        # `--near` filters against the gps fixes in the selected window (lon, lat).
        gps_lonlat = [(row[3], row[2]) for row in gps]
        log_transport(transport, gps_lonlat=gps_lonlat, near=args.near)
    blueprint = build_blueprint(devices, has_transport=bool(transport))

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
        f"+ {len(transport)} transport features across {len(devices)} device(s)"
    )


if __name__ == "__main__":
    main()
