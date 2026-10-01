import pyarrow as pa
import pyarrow.parquet as pq
import pytest
import shapely

import lookout_medallion
from conftest import BERLIN

EXTRACTS = {"20260722T090000Z": "DE", "20260723T090000Z": "GB"}


def write_parquet(path, table):
    path.parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(table, path)


@pytest.fixture
def filled(store):
    write_parquet(
        store / "bronze" / "extract_manifest" / "part-0.parquet",
        pa.table(
            {
                "extract_id": pa.array(list(EXTRACTS), pa.string()),
                "country": pa.array(list(EXTRACTS.values()), pa.string()),
                "release": pa.array(["2026-07-22.0"] * 2, pa.string()),
            }
        ),
    )
    for extract_id in EXTRACTS:
        for theme, of_type in [("base", "water"), ("transportation", "segment")]:
            write_parquet(
                store
                / "bronze"
                / "overture_extract"
                / f"extract_id={extract_id}"
                / f"theme={theme}"
                / f"type={of_type}"
                / "part-0.parquet",
                pa.table(
                    {
                        "id": pa.array([f"{of_type}-{extract_id}"], pa.string()),
                        "extract_id": pa.array([extract_id], pa.string()),
                        "geometry": pa.array(
                            [shapely.to_wkb(shapely.Point(BERLIN))], pa.binary()
                        ),
                    }
                ),
            )
    write_parquet(
        store / "bronze" / "gps_reading" / "ingested_date=2026-07-22" / "part-0.parquet",
        pa.table({"device_id": pa.array(["m5-1"], pa.string())}),
    )
    return store


def query(filled, sql, **kwargs):
    return pa.table(lookout_medallion.query_bronze(sql, root=str(filled), **kwargs))


def test_a_dataset_is_read_under_its_own_name(filled):
    table = query(filled, "SELECT country FROM extract_manifest ORDER BY country")

    assert table.column("country").to_pylist() == ["DE", "GB"]


def test_a_datasets_own_partition_comes_back_as_a_column(filled):
    table = query(filled, "SELECT device_id, ingested_date FROM gps_reading")

    assert table.column("ingested_date").to_pylist() == ["2026-07-22"]


def test_a_theme_and_type_covers_every_extract_taken(filled):
    table = query(
        filled,
        "SELECT id FROM water ORDER BY id",
        tables={"water": {"theme": "base", "type": "water"}},
    )

    assert table.column("id").to_pylist() == [
        f"water-{extract_id}" for extract_id in EXTRACTS
    ]


def test_an_extract_joins_to_the_manifest_that_recorded_it(filled):
    table = query(
        filled,
        """
        SELECT s.id FROM segment s JOIN extract_manifest m USING (extract_id)
        WHERE m.country = 'GB'
        """,
        tables={"segment": {"theme": "transportation", "type": "segment"}},
    )

    assert table.column("id").to_pylist() == ["segment-20260723T090000Z"]


def test_geometry_comes_back_as_the_wkb_the_extract_holds(filled):
    table = query(
        filled,
        "SELECT geometry FROM water LIMIT 1",
        tables={"water": {"theme": "base", "type": "water"}},
    )

    assert shapely.from_wkb(table.column("geometry")[0].as_py()) == shapely.Point(BERLIN)


def test_the_extracts_cannot_be_read_without_naming_a_theme_and_type(filled):
    with pytest.raises(ValueError, match="theme"):
        query(filled, "SELECT id FROM overture_extract")


def test_a_theme_no_extract_holds_is_reported(filled):
    with pytest.raises(ValueError, match="theme=places"):
        query(
            filled,
            "SELECT id FROM places",
            tables={"places": {"theme": "places", "type": "place"}},
        )


def test_a_dataset_bronze_does_not_hold_is_refused_with_the_known_ones(filled):
    with pytest.raises(ValueError, match="water_crossing"):
        query(filled, "SELECT * FROM water_crossing")
