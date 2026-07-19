import argparse
import sqlite3

import pytest
import shapely

import main


class TestParseSince:
    @pytest.mark.parametrize(
        "text, seconds",
        [("45s", 45), ("30m", 1800), ("12h", 43200), ("7d", 604800), ("2w", 1209600)],
    )
    def test_valid(self, text, seconds):
        assert main.parse_since(text) == seconds

    @pytest.mark.parametrize("text", ["7days", "d", "", "-5d", "1.5d", "7 d", "7D"])
    def test_invalid(self, text):
        with pytest.raises(argparse.ArgumentTypeError):
            main.parse_since(text)


class TestDeviceFilter:
    def test_no_devices_is_unrestricted(self):
        assert main._device_filter(None) == ("", [])
        assert main._device_filter([]) == ("", [])

    def test_prefix_terms_and_params(self):
        clause, params = main._device_filter(["abc", "de"])
        assert clause.count("substr(device_id, 1, length(?)) = ?") == 2
        assert " OR " in clause
        # each prefix contributes the length arg and the comparison arg.
        assert params == ["abc", "abc", "de", "de"]


class TestFetchQueries:
    """The fetch functions against an in-memory DB with the recorder's schema."""

    @pytest.fixture
    def conn(self):
        conn = sqlite3.connect(":memory:")
        conn.executescript(
            """
            CREATE TABLE accel (device_id TEXT, t INTEGER, rms REAL, peak REAL, n INTEGER, x REAL, y REAL, z REAL);
            CREATE TABLE gps (device_id TEXT, t INTEGER, lat REAL, lon REAL, alt REAL, acc REAL, speed REAL, heading REAL);
            """
        )
        rows = [
            ("aaaa-1", 1000, 0.1, 0.2, 0.3),
            ("aaaa-1", 2000, 0.4, 0.5, 0.6),
            ("bbbb-2", 2000, 1.0, 1.0, 1.0),
        ]
        conn.executemany("INSERT INTO accel (device_id, t, x, y, z) VALUES (?, ?, ?, ?, ?)", rows)
        conn.executemany(
            "INSERT INTO gps (device_id, t, lat, lon, alt, acc) VALUES (?, ?, ?, ?, ?, ?)",
            [("aaaa-1", 1500, 55.9, -3.1, 80.0, 5.0), ("bbbb-2", 500, 51.5, -0.1, 10.0, 8.0)],
        )
        conn.commit()
        yield conn
        conn.close()

    def test_fetch_accel_selects_aggregate_columns(self, conn):
        conn.execute(
            "INSERT INTO accel (device_id, t, rms, peak, n) VALUES ('cccc-3', 3000, 0.42, 1.7, 600)"
        )
        conn.commit()
        (row,) = [r for r in main.fetch_accel(conn, 0, ["cccc"])]
        # (device_id, t, rms, peak, n, x, y, z)
        assert row[2] == 0.42 and row[3] == 1.7 and row[4] == 600

    def test_fetch_gps_selects_speed_and_heading(self, conn):
        conn.execute(
            "INSERT INTO gps (device_id, t, lat, lon, acc, speed, heading) "
            "VALUES ('cccc-3', 3000, 55.9, -3.1, 5.0, 31.4, 275.0)"
        )
        conn.commit()
        (row,) = [r for r in main.fetch_gps(conn, 0, ["cccc"])]
        # (device_id, t, lat, lon, acc, speed, heading)
        assert row[5] == 31.4 and row[6] == 275.0

    def test_cutoff_excludes_older(self, conn):
        rows = main.fetch_accel(conn, cutoff_ms=1500, devices=None)
        assert [r[1] for r in rows] == [2000, 2000]

    def test_prefix_selects_matching_device(self, conn):
        rows = main.fetch_accel(conn, cutoff_ms=0, devices=["aaaa"])
        assert {r[0] for r in rows} == {"aaaa-1"}
        assert len(rows) == 2

    def test_short_prefix_can_match_multiple(self, conn):
        # A prefix shared by several ids selects them all.
        gps = main.fetch_gps(conn, cutoff_ms=0, devices=["a", "b"])
        assert {r[0] for r in gps} == {"aaaa-1", "bbbb-2"}

    def test_results_ordered_by_time(self, conn):
        rows = main.fetch_accel(conn, cutoff_ms=0, devices=None)
        assert [r[1] for r in rows] == sorted(r[1] for r in rows)


class TestSpeedColor:
    def test_none_speed_is_grey(self):
        assert main._speed_color(None, 0.0, 30.0) == main._NO_SPEED

    def test_endpoints_hit_ramp_extremes(self):
        assert main._speed_color(0.0, 0.0, 30.0) == main._VIRIDIS[0][1]
        assert main._speed_color(30.0, 0.0, 30.0) == main._VIRIDIS[-1][1]

    def test_degenerate_range_is_midpoint(self):
        # All fixes at the same speed: no gradient, pick the ramp's middle.
        assert main._speed_color(5.0, 5.0, 5.0) == main._viridis(0.5)

    def test_returns_rgb_triple(self):
        r, g, b = main._speed_color(15.0, 0.0, 30.0)
        assert all(0 <= c <= 255 for c in (r, g, b))


class TestTransport:
    """The `transport` fetch/transform path: reading the enriched table and turning
    its WKB geometry into rerun-ready, class-coloured, lat/lon geometry."""

    def test_fetch_missing_table_is_empty(self):
        conn = sqlite3.connect(":memory:")
        assert main.fetch_transport(conn) == []
        conn.close()

    def test_fetch_reads_rows(self):
        conn = sqlite3.connect(":memory:")
        conn.execute(
            "CREATE TABLE transport (gers_id TEXT, kind TEXT, class TEXT, geom BLOB)"
        )
        conn.execute(
            "INSERT INTO transport (gers_id, kind, class, geom) VALUES ('c1', 'connector', NULL, ?)",
            [shapely.Point(11.0, 50.0).wkb],
        )
        conn.commit()
        rows = main.fetch_transport(conn)
        conn.close()
        assert [(r[0], r[1]) for r in rows] == [("connector", None)]

    def test_class_color_maps_known_and_defaults_unknown(self):
        assert main._class_color("tram") == main._CLASS_COLORS["tram"]
        assert main._class_color("unknown") == main._CLASS_DEFAULT
        assert main._class_color(None) == main._CLASS_DEFAULT

    def test_segment_flips_to_lat_lon_and_colours_by_class(self):
        geom = shapely.LineString([(11.0, 50.0), (11.1, 50.1)]).wkb
        segments, colors, connectors = main._transport_geometry([("segment", "tram", geom)])
        # WKB is (lon, lat); rerun wants (lat, lon).
        assert segments == [[(50.0, 11.0), (50.1, 11.1)]]
        assert colors == [main._CLASS_COLORS["tram"]]
        assert connectors == []

    def test_connector_point_flips_to_lat_lon(self):
        geom = shapely.Point(11.0, 50.0).wkb
        segments, _colors, connectors = main._transport_geometry([("connector", None, geom)])
        assert connectors == [(50.0, 11.0)]
        assert segments == []

    def test_multilinestring_splits_into_one_polyline_per_part(self):
        geom = shapely.MultiLineString(
            [[(11.0, 50.0), (11.1, 50.1)], [(12.0, 51.0), (12.1, 51.1)]]
        ).wkb
        segments, colors, _ = main._transport_geometry([("segment", "tram", geom)])
        assert len(segments) == 2
        assert colors == [main._CLASS_COLORS["tram"]] * 2

    def test_near_keeps_close_segments_and_drops_far_ones(self):
        close = shapely.LineString([(11.0, 50.0), (11.01, 50.01)]).wkb
        far = shapely.LineString([(20.0, 60.0), (20.1, 60.1)]).wkb
        rows = [("segment", "tram", close), ("segment", "tram", far)]
        segments, _, _ = main._transport_geometry(
            rows, gps_lonlat=[(11.0, 50.0)], near=0.05
        )
        # Only the segment within 0.05 degrees of the fix survives.
        assert segments == [[(50.0, 11.0), (50.01, 11.01)]]

    def test_near_none_keeps_all_segments(self):
        far = shapely.LineString([(20.0, 60.0), (20.1, 60.1)]).wkb
        segments, _, _ = main._transport_geometry(
            [("segment", "tram", far)], gps_lonlat=[(11.0, 50.0)], near=None
        )
        assert len(segments) == 1


class TestTrains:
    """The `train_segment` fetch/transform path: reading the ingested table and turning
    its WKB legs + realtime times into interpolated, mode-coloured moving-dot samples."""

    def test_fetch_missing_table_is_empty(self):
        conn = sqlite3.connect(":memory:")
        assert main.fetch_train_segments(conn, 0) == []
        conn.close()

    @pytest.fixture
    def conn(self):
        conn = sqlite3.connect(":memory:")
        conn.execute(
            "CREATE TABLE train_segment (trip_id TEXT, mode TEXT, route_color TEXT, "
            "departure INTEGER, arrival INTEGER, geom BLOB)"
        )
        yield conn
        conn.close()

    def _insert(self, conn, trip_id, mode, route_color, departure, arrival, line):
        conn.execute(
            "INSERT INTO train_segment (trip_id, mode, route_color, departure, arrival, geom) "
            "VALUES (?, ?, ?, ?, ?, ?)",
            [trip_id, mode, route_color, departure, arrival, line.wkb],
        )
        conn.commit()

    def test_fetch_windows_by_arrival(self, conn):
        line = shapely.LineString([(11.0, 50.0), (11.0, 52.0)])
        self._insert(conn, "old", "REGIONAL_RAIL", None, 100, 500, line)
        self._insert(conn, "new", "REGIONAL_RAIL", None, 1000, 2000, line)
        rows = main.fetch_train_segments(conn, cutoff_ms=600)
        assert [r[0] for r in rows] == ["new"], "leg arriving before the cutoff is dropped"

    def test_hex_rgb_parses_and_rejects(self):
        assert main._hex_rgb("ff8800") == (255, 136, 0)
        assert main._hex_rgb("#ff8800") == (255, 136, 0)
        assert main._hex_rgb("nope") is None
        assert main._hex_rgb("fff") is None

    def test_train_color_prefers_route_color_then_mode(self):
        assert main._train_color("TRAM", "ff8800") == (255, 136, 0)
        assert main._train_color("TRAM", None) == main._MODE_COLORS["TRAM"]
        assert main._train_color("TRAM", "") == main._MODE_COLORS["TRAM"]
        assert main._train_color("WHAT", None) == main._MODE_DEFAULT

    def test_interpolates_position_and_flips_to_lat_lon(self):
        # A north-running 2-point line (lon fixed at 11, lat 50→52) over [1000, 3000] ms.
        line = shapely.LineString([(11.0, 50.0), (11.0, 52.0)])
        rows = [("trip-1", "REGIONAL_RAIL", None, 1000, 3000, line.wkb)]
        trains = main._train_samples(rows, step_s=1)
        samples = trains["trip-1"]["samples"]
        # Endpoints plus the midpoint at half the span; WKB (lon, lat) → (lat, lon).
        assert (1.0, 50.0, 11.0) in samples  # frac 0.0 → start
        assert (2.0, 51.0, 11.0) in samples  # frac 0.5 → midpoint
        assert (3.0, 52.0, 11.0) in samples  # frac 1.0 → end
        assert trains["trip-1"]["color"] == main._MODE_COLORS["REGIONAL_RAIL"]

    def test_samples_are_time_ordered_across_legs(self):
        line_a = shapely.LineString([(11.0, 50.0), (11.0, 51.0)])
        line_b = shapely.LineString([(11.0, 51.0), (11.0, 52.0)])
        rows = [
            ("trip-1", "REGIONAL_RAIL", None, 3000, 4000, line_b.wkb),
            ("trip-1", "REGIONAL_RAIL", None, 1000, 2000, line_a.wkb),
        ]
        trains = main._train_samples(rows, step_s=1)
        times = [t for t, _, _ in trains["trip-1"]["samples"]]
        assert times == sorted(times)
        assert len(trains["trip-1"]["route"]) == 2, "one route polyline per leg"

    def test_zero_duration_leg_yields_single_start_sample(self):
        line = shapely.LineString([(11.0, 50.0), (11.0, 52.0)])
        rows = [("trip-1", "REGIONAL_RAIL", None, 1000, 1000, line.wkb)]
        samples = main._train_samples(rows, step_s=1)["trip-1"]["samples"]
        assert samples == [(1.0, 50.0, 11.0)]


class TestReportedPrefixScenario:
    """Reproduces `--devices 77a`: with several full uuids present, a short prefix
    must select *only* the id it is a prefix of and exclude every other device."""

    UUIDS = [
        "77a64f88-c65f-4f9f-90bb-0d069b9f55a1",
        "c7af4ca1-860d-428f-9b0c-c879985651fb",
        "cdf2ab5a-0157-4dbe-8406-27a7d4c4970d",
    ]

    @pytest.fixture
    def conn(self):
        conn = sqlite3.connect(":memory:")
        conn.executescript(
            """
            CREATE TABLE accel (device_id TEXT, t INTEGER, rms REAL, peak REAL, n INTEGER, x REAL, y REAL, z REAL);
            CREATE TABLE gps (device_id TEXT, t INTEGER, lat REAL, lon REAL, alt REAL, acc REAL, speed REAL, heading REAL);
            """
        )
        for uuid in self.UUIDS:
            conn.execute("INSERT INTO accel (device_id, t, x, y, z) VALUES (?, 1000, 0.0, 0.0, 0.0)", [uuid])
            conn.execute("INSERT INTO gps (device_id, t, lat, lon, alt, acc) VALUES (?, 1000, 0.0, 0.0, 0.0, 0.0)", [uuid])
        conn.commit()
        yield conn
        conn.close()

    def test_prefix_excludes_other_devices(self, conn):
        accel = main.fetch_accel(conn, cutoff_ms=0, devices=["77a"])
        gps = main.fetch_gps(conn, cutoff_ms=0, devices=["77a"])
        selected = {r[0] for r in accel} | {r[0] for r in gps}
        assert selected == {"77a64f88-c65f-4f9f-90bb-0d069b9f55a1"}
