import argparse
import datetime as dt

import pytest

import main

# A fixed instant the fixtures are built around, in epoch milliseconds.
NOW_MS = 1_785_000_000_000
MINUTE_MS = 60_000
DAY_MS = 86_400_000


def hive_date(t_ms: int) -> str:
    """The UTC date `t_ms` falls on, as a hive partition value."""
    return dt.datetime.fromtimestamp(t_ms / 1000.0, dt.timezone.utc).date().isoformat()


def write_partition(store, layer: str, dataset: str, key: str, value: str, rows: list[str]) -> None:
    """Write `rows` as one file in one partition of `dataset`, the shape every writer into
    this store produces: one file per write, under a `key=value` directory."""
    directory = store.root / layer / dataset / f"{key}={value}"
    directory.mkdir(parents=True, exist_ok=True)
    select = " UNION ALL ".join(f"SELECT {row}" for row in rows)
    store.con.execute(
        f"COPY ({select}) TO '{directory / f'{value}.parquet'}' (FORMAT parquet)"
    )


def gps(device_id: str, t_ms: int, lat=50.0, lon=8.0, acc=5.0, speed=None) -> str:
    speed_sql = "NULL::DOUBLE" if speed is None else f"{speed}::DOUBLE"
    return (
        f"'{device_id}' AS device_id, to_timestamp({t_ms} / 1000.0) AS t, "
        f"{lat}::DOUBLE AS lat, {lon}::DOUBLE AS lon, {acc}::DOUBLE AS acc, "
        f"{speed_sql} AS speed"
    )


def accel(device_id: str, t_ms: int, rms=0.42, peak=1.7, n=600) -> str:
    return (
        f"'{device_id}' AS device_id, to_timestamp({t_ms} / 1000.0) AS t, "
        f"{rms}::DOUBLE AS rms, {peak}::DOUBLE AS peak, {n}::UINTEGER AS n"
    )


def leg(
    trip_id: str,
    departure_ms: int,
    arrival_ms: int,
    line="LINESTRING(11 50, 11 52)",
    mode="REGIONAL_RAIL",
    route_color=None,
    route_name=None,
    train_number=None,
) -> str:
    def text(value):
        return "NULL::VARCHAR" if value is None else f"'{value}'"

    number = "NULL::UINTEGER" if train_number is None else f"{train_number}::UINTEGER"
    return (
        f"'{trip_id}' AS trip_id, '{mode}' AS mode, {text(route_color)} AS route_color, "
        f"{text(route_name)} AS route_name, {number} AS train_number, "
        f"to_timestamp({departure_ms} / 1000.0) AS departure, "
        f"to_timestamp({arrival_ms} / 1000.0) AS arrival, "
        f"ST_GeomFromText('{line}') AS geometry"
    )


def write_readings(store, rows: list[str], dataset: str, ingested_ms: int = NOW_MS) -> None:
    write_partition(
        store, main.BRONZE, dataset, "ingested_date", hive_date(ingested_ms), rows
    )


def write_legs(store, rows: list[str], departure_ms: int = NOW_MS) -> None:
    write_partition(
        store, main.SILVER, "train_segment", "departure_date", hive_date(departure_ms), rows
    )


@pytest.fixture
def store(tmp_path):
    return main.Store(tmp_path)


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


class TestUnwrittenDatasets:
    """A store where a dataset has never been written is not an error the caller has to
    distinguish: it reads as a store holding none of that data."""

    def test_missing_sensor_datasets_read_as_empty(self, store):
        assert main.fetch_gps(store, NOW_MS, None) == []
        assert main.fetch_accel(store, NOW_MS, None) == []

    def test_missing_train_dataset_reads_as_empty(self, store):
        assert main.fetch_train_legs(store, NOW_MS) == []
        assert main.fetch_train_positions(store, NOW_MS) == []


class TestSensorQueries:
    """The bronze sensor reads: the time window, the device filter, and the columns each
    dataset contributes."""

    def test_gps_columns_come_back_as_epoch_millis_and_lat_lon(self, store):
        write_readings(
            store,
            [gps("aaaa-1", NOW_MS - MINUTE_MS, lat=55.9, lon=-3.1, acc=8.0, speed=31.4)],
            "gps_reading",
        )

        (row,) = main.fetch_gps(store, NOW_MS - DAY_MS, None)

        assert row == ("aaaa-1", NOW_MS - MINUTE_MS, 55.9, -3.1, 8.0, 31.4)

    def test_accel_selects_the_aggregate_columns(self, store):
        write_readings(
            store,
            [accel("aaaa-1", NOW_MS - MINUTE_MS, rms=0.42, peak=1.7, n=600)],
            "accel_reading",
        )

        (row,) = main.fetch_accel(store, NOW_MS - DAY_MS, None)

        assert row == ("aaaa-1", NOW_MS - MINUTE_MS, 0.42, 1.7, 600)

    def test_an_accel_reading_that_aggregated_nothing_is_excluded(self, store):
        """A reading with `n = 0` carries no ride signal — its zeroed aggregates would plot
        as a real flat zero. Legacy readings predating the aggregates look the same."""
        write_readings(
            store,
            [
                accel("aaaa-1", NOW_MS - MINUTE_MS, rms=0.0, peak=0.0, n=0),
                accel("aaaa-1", NOW_MS),
            ],
            "accel_reading",
        )

        rows = main.fetch_accel(store, NOW_MS - DAY_MS, None)

        assert [row[1] for row in rows] == [NOW_MS]

    def test_a_reading_before_the_cutoff_is_excluded(self, store):
        write_readings(
            store,
            [accel("aaaa-1", NOW_MS - 2 * MINUTE_MS), accel("aaaa-1", NOW_MS)],
            "accel_reading",
        )

        rows = main.fetch_accel(store, NOW_MS - MINUTE_MS, None)

        assert [row[1] for row in rows] == [NOW_MS]

    def test_readings_are_ordered_by_time(self, store):
        write_readings(
            store,
            [
                gps("aaaa-1", NOW_MS),
                gps("aaaa-1", NOW_MS - 2 * MINUTE_MS),
                gps("bbbb-2", NOW_MS - MINUTE_MS),
            ],
            "gps_reading",
        )

        rows = main.fetch_gps(store, NOW_MS - DAY_MS, None)

        assert [row[1] for row in rows] == sorted(row[1] for row in rows)

    def test_no_devices_selects_every_device(self, store):
        write_readings(
            store, [gps("aaaa-1", NOW_MS), gps("bbbb-2", NOW_MS)], "gps_reading"
        )

        for devices in (None, []):
            rows = main.fetch_gps(store, NOW_MS - DAY_MS, devices)
            assert {row[0] for row in rows} == {"aaaa-1", "bbbb-2"}

    def test_a_prefix_selects_only_the_ids_it_starts(self, store):
        write_readings(
            store,
            [gps("aaaa-1", NOW_MS), gps("bbbb-2", NOW_MS), gps("cccc-3", NOW_MS)],
            "gps_reading",
        )

        rows = main.fetch_gps(store, NOW_MS - DAY_MS, ["aaaa"])

        assert {row[0] for row in rows} == {"aaaa-1"}

    def test_a_short_prefix_can_match_several_devices(self, store):
        write_readings(
            store,
            [gps("aaaa-1", NOW_MS), gps("aaab-2", NOW_MS), gps("bbbb-3", NOW_MS)],
            "gps_reading",
        )

        rows = main.fetch_gps(store, NOW_MS - DAY_MS, ["aaa", "zzz"])

        assert {row[0] for row in rows} == {"aaaa-1", "aaab-2"}


class TestReportedPrefixScenario:
    """Reproduces `--devices 77a`: with several full uuids present, a short prefix must
    select *only* the id it is a prefix of and exclude every other device."""

    UUIDS = [
        "77a64f88-c65f-4f9f-90bb-0d069b9f55a1",
        "c7af4ca1-860d-428f-9b0c-c879985651fb",
        "cdf2ab5a-0157-4dbe-8406-27a7d4c4970d",
    ]

    def test_prefix_excludes_other_devices(self, store):
        write_readings(store, [gps(uuid, NOW_MS) for uuid in self.UUIDS], "gps_reading")
        write_readings(
            store, [accel(uuid, NOW_MS) for uuid in self.UUIDS], "accel_reading"
        )

        selected = {row[0] for row in main.fetch_gps(store, NOW_MS - DAY_MS, ["77a"])} | {
            row[0] for row in main.fetch_accel(store, NOW_MS - DAY_MS, ["77a"])
        }

        assert selected == {"77a64f88-c65f-4f9f-90bb-0d069b9f55a1"}


class TestPartitioning:
    """The time window is a predicate over a hive partition key, so what that key reads
    back as, and how much of the dataset it makes the engine open, are properties the
    queries depend on."""

    def test_a_date_partition_key_reads_back_as_a_date(self, store):
        write_readings(store, [gps("aaaa-1", NOW_MS)], "gps_reading")

        assert store.rows(
            main.BRONZE,
            "gps_reading",
            "SELECT DISTINCT typeof(ingested_date) FROM {dataset}",
        ) == [("DATE",)]

    def test_the_window_scans_only_the_partitions_it_covers(self, store):
        for day in range(3):
            ingested_ms = NOW_MS - day * DAY_MS
            write_readings(store, [gps("aaaa-1", ingested_ms)], "gps_reading", ingested_ms)

        plan = self.plan(store, NOW_MS - DAY_MS)

        assert "Scanning Files: 2/3" in plan, plan

    @staticmethod
    def plan(store, cutoff_ms: int) -> str:
        """DuckDB's plan for the gps query, which reports how many of the dataset's files
        it scans."""
        cutoff = dt.datetime.fromtimestamp(cutoff_ms / 1000.0, dt.timezone.utc)
        source = "read_parquet($dataset, hive_partitioning = 1)"
        glob = store.root / main.BRONZE / "gps_reading" / "**" / "*.parquet"
        (row,) = store.con.execute(
            "EXPLAIN " + main._GPS.format(dataset=source),
            {
                "dataset": str(glob),
                "cutoff_s": cutoff.timestamp(),
                "cutoff_date": cutoff.date(),
                "devices": None,
            },
        ).fetchall()
        return row[1]


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


class TestTrainLegs:
    """The silver `train_segment` read: the window over the legs, and their geometry
    arriving as coordinates rather than as something to decode here."""

    def test_a_leg_arriving_before_the_cutoff_is_excluded(self, store):
        write_legs(
            store,
            [
                leg("old", NOW_MS - 4 * MINUTE_MS, NOW_MS - 3 * MINUTE_MS),
                leg("new", NOW_MS - MINUTE_MS, NOW_MS),
            ],
        )

        rows = main.fetch_train_legs(store, NOW_MS - 2 * MINUTE_MS)

        assert [row[0] for row in rows] == ["new"]

    def test_a_leg_departing_the_day_before_the_cutoff_is_kept(self, store):
        """A leg's partition is its departure date, so one that departed before the window
        and is still running when it starts must not be pruned away."""
        departure_ms = NOW_MS - DAY_MS
        write_legs(store, [leg("overnight", departure_ms, NOW_MS)], departure_ms)

        rows = main.fetch_train_legs(store, NOW_MS - MINUTE_MS)

        assert [row[0] for row in rows] == ["overnight"]

    def test_the_route_comes_back_as_lat_lon_vertices(self, store):
        """Stored geometry is lon/lat; rerun wants (lat, lon)."""
        write_legs(store, [leg("trip-1", NOW_MS - MINUTE_MS, NOW_MS)])

        (row,) = main.fetch_train_legs(store, NOW_MS - DAY_MS)

        assert row[5] == [[50.0, 11.0], [52.0, 11.0]]

    def test_a_trip_carries_its_colour_label_and_route(self, store):
        write_legs(
            store,
            [
                leg(
                    "ice",
                    NOW_MS - MINUTE_MS,
                    NOW_MS,
                    mode="HIGHSPEED_RAIL",
                    route_name="55",
                    train_number=2569,
                )
            ],
        )

        trains = main._trains(main.fetch_train_legs(store, NOW_MS - DAY_MS))

        assert trains["ice"]["color"] == main._MODE_COLORS["HIGHSPEED_RAIL"]
        assert trains["ice"]["entity"] == "trains/2569/ice"
        assert trains["ice"]["route"] == [[[50.0, 11.0], [52.0, 11.0]]]

    def test_each_leg_of_a_trip_is_its_own_route_polyline(self, store):
        write_legs(
            store,
            [
                leg("trip-1", NOW_MS - 2 * MINUTE_MS, NOW_MS - MINUTE_MS),
                leg(
                    "trip-1",
                    NOW_MS - MINUTE_MS,
                    NOW_MS,
                    line="LINESTRING(11 52, 11 54)",
                ),
            ],
        )

        trains = main._trains(main.fetch_train_legs(store, NOW_MS - DAY_MS))

        assert len(trains["trip-1"]["route"]) == 2


class TestTrainPositions:
    """The moving dot: each leg resampled along its route by its realtime-corrected
    times."""

    def test_endpoints_and_steps_are_interpolated_along_the_route(self, store):
        # A north-running 2-point line (lon 11, lat 50→52) over two minutes.
        write_legs(store, [leg("trip-1", NOW_MS - 2 * MINUTE_MS, NOW_MS)])

        rows = main.fetch_train_positions(store, NOW_MS - DAY_MS, step_s=60)

        assert rows == [
            ("trip-1", NOW_MS - 2 * MINUTE_MS, 50.0, 11.0),
            ("trip-1", NOW_MS - MINUTE_MS, 51.0, 11.0),
            ("trip-1", NOW_MS, 52.0, 11.0),
        ]

    def test_a_span_shorter_than_the_step_still_ends_at_the_arrival(self, store):
        write_legs(store, [leg("trip-1", NOW_MS - MINUTE_MS, NOW_MS)])

        rows = main.fetch_train_positions(store, NOW_MS - DAY_MS, step_s=3600)

        assert [row[1] for row in rows] == [NOW_MS - MINUTE_MS, NOW_MS]

    def test_a_zero_duration_leg_yields_a_single_start_position(self, store):
        write_legs(store, [leg("trip-1", NOW_MS, NOW_MS)])

        rows = main.fetch_train_positions(store, NOW_MS - DAY_MS, step_s=60)

        assert rows == [("trip-1", NOW_MS, 50.0, 11.0)]

    def test_positions_are_ordered_by_time_across_a_trips_legs(self, store):
        write_legs(
            store,
            [
                leg(
                    "trip-1",
                    NOW_MS - MINUTE_MS,
                    NOW_MS,
                    line="LINESTRING(11 52, 11 54)",
                ),
                leg("trip-1", NOW_MS - 3 * MINUTE_MS, NOW_MS - 2 * MINUTE_MS),
            ],
        )

        rows = main.fetch_train_positions(store, NOW_MS - DAY_MS, step_s=60)

        times = [row[1] for row in rows]
        assert times == sorted(times)


class TestTrainIdentity:
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
        # DELFI's mode separates long-distance directly — no agency needed.
        assert main._train_color("HIGHSPEED_RAIL", None) == main._MODE_COLORS["HIGHSPEED_RAIL"]

    def test_train_label_prefers_number_then_line(self):
        assert main._train_label(2569, "55") == "2569"
        assert main._train_label(None, "RE4") == "RE4"
        assert main._train_label(None, None) is None

    def test_train_entity_groups_by_label_but_stays_unique(self):
        # Label groups the path; trip_id leaf keeps distinct trips from colliding.
        assert main._train_entity("trip-a", "2569") == "trains/2569/trip-a"
        assert main._train_entity("trip-b", None) == "trains/trip-b"
        # Slashes/spaces in a line name don't spawn extra path levels.
        assert main._train_entity("trip-c", "S1/S11") == "trains/S1-S11/trip-c"


class TestStoreLocation:
    def workspace(self, directory):
        """A directory holding a workspace manifest, as the app's own root does."""
        (directory / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*"]\n')
        return directory

    def test_the_store_is_found_from_the_workspace_root_itself(self, tmp_path, monkeypatch):
        root = self.workspace(tmp_path)
        monkeypatch.chdir(root)
        assert main._medallion_root_in_repo() == root / "data/medallion"

    def test_the_store_is_found_from_below_the_workspace_root(self, tmp_path, monkeypatch):
        root = self.workspace(tmp_path)
        deep = root / "notebooks" / "sessions"
        deep.mkdir(parents=True)
        monkeypatch.chdir(deep)
        assert main._medallion_root_in_repo() == root / "data/medallion"

    def test_no_workspace_above_says_so_rather_than_stopping_short(self, tmp_path, monkeypatch):
        outside = tmp_path / "outside"
        outside.mkdir()
        monkeypatch.chdir(outside)
        with pytest.raises(FileNotFoundError, match="Cargo.toml declaring"):
            main._medallion_root_in_repo()
