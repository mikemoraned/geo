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
