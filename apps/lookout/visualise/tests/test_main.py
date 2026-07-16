import argparse
import sqlite3

import pytest

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
            CREATE TABLE accel (device_id TEXT, t INTEGER, x REAL, y REAL, z REAL);
            CREATE TABLE gps (device_id TEXT, t INTEGER, lat REAL, lon REAL, alt REAL, acc REAL);
            """
        )
        rows = [
            ("aaaa-1", 1000, 0.1, 0.2, 0.3),
            ("aaaa-1", 2000, 0.4, 0.5, 0.6),
            ("bbbb-2", 2000, 1.0, 1.0, 1.0),
        ]
        conn.executemany("INSERT INTO accel VALUES (?, ?, ?, ?, ?)", rows)
        conn.executemany(
            "INSERT INTO gps VALUES (?, ?, ?, ?, ?, ?)",
            [("aaaa-1", 1500, 55.9, -3.1, 80.0, 5.0), ("bbbb-2", 500, 51.5, -0.1, 10.0, 8.0)],
        )
        conn.commit()
        yield conn
        conn.close()

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
            CREATE TABLE accel (device_id TEXT, t INTEGER, x REAL, y REAL, z REAL);
            CREATE TABLE gps (device_id TEXT, t INTEGER, lat REAL, lon REAL, alt REAL, acc REAL);
            """
        )
        for uuid in self.UUIDS:
            conn.execute("INSERT INTO accel VALUES (?, 1000, 0.0, 0.0, 0.0)", [uuid])
            conn.execute("INSERT INTO gps VALUES (?, 1000, 0.0, 0.0, 0.0, 0.0)", [uuid])
        conn.commit()
        yield conn
        conn.close()

    def test_prefix_excludes_other_devices(self, conn):
        accel = main.fetch_accel(conn, cutoff_ms=0, devices=["77a"])
        gps = main.fetch_gps(conn, cutoff_ms=0, devices=["77a"])
        selected = {r[0] for r in accel} | {r[0] for r in gps}
        assert selected == {"77a64f88-c65f-4f9f-90bb-0d069b9f55a1"}
