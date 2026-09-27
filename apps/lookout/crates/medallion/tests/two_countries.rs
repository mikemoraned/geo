use std::sync::Arc;

use arrow::array::{Int64Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use geo_types::Point;
use medallion::{
    COUNTRY, Country, DatasetSpec, GEOMETRY, PROJECTED_GEOMETRY, Projector, Query, Root, layers,
    projected_wkb_field, wkb_column, wkb_field,
};

const CROSSINGS: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("water_crossing", COUNTRY);

const BERLIN: (f64, f64) = (13.404954, 52.520008);
const EDINBURGH: (f64, f64) = (-3.188267, 55.953251);

fn crossings_of(country: Country, at: (f64, f64)) -> RecordBatch {
    let point = Point::new(at.0, at.1);
    let projected = Projector::for_country(country)
        .expect("a projector")
        .project(&point)
        .expect("project the point");

    let (geometry_field, geometry) =
        wkb_column(wkb_field(GEOMETRY).expect("a field"), &[point]).expect("a geometry column");
    let (projected_field, projected) = wkb_column(
        projected_wkb_field(PROJECTED_GEOMETRY, country).expect("a field"),
        &[projected],
    )
    .expect("a projected column");

    RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Arc::new(Field::new("id", DataType::Int64, false)),
            geometry_field,
            projected_field,
        ])),
        vec![Arc::new(Int64Array::from(vec![1])), geometry, projected],
    )
    .expect("a batch")
}

async fn store_of_two_countries() -> (tempfile::TempDir, Root) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    for (country, at) in [
        (Country::Germany, BERLIN),
        (Country::UnitedKingdom, EDINBURGH),
    ] {
        root.dataset(CROSSINGS)
            .partition(COUNTRY, country)
            .expect("a country partition")
            .replace_with_geo(&[crossings_of(country, at)])
            .await
            .expect("write the country's rows");
    }
    (tmp, root)
}

#[tokio::test]
async fn a_dataset_written_in_two_zones_is_not_read_in_one_scan() {
    let (_tmp, root) = store_of_two_countries().await;
    let query = Query::new(root.clone());

    let registered = query.register_by_name(CROSSINGS).await;

    assert!(
        registered.is_err(),
        "a projected column carries one zone per country, so the files of two countries \
         describe that column differently and no one scan spans them"
    );
}

#[tokio::test]
async fn the_countries_of_a_dataset_are_the_ones_it_holds_partitions_for() {
    let (_tmp, root) = store_of_two_countries().await;

    let countries = medallion::countries_of(&root, CROSSINGS)
        .await
        .expect("the countries the dataset holds");

    assert_eq!(countries, [Country::Germany, Country::UnitedKingdom]);
}

#[tokio::test]
async fn a_partition_naming_a_country_with_no_zone_is_reported_rather_than_read() {
    let (_tmp, root) = store_of_two_countries().await;
    root.dataset(CROSSINGS)
        .partition(COUNTRY, "FR")
        .expect("a country partition")
        .replace_with_geo(&[crossings_of(Country::Germany, BERLIN)])
        .await
        .expect("write a country silver has no zone for");

    let err = medallion::countries_of(&root, CROSSINGS)
        .await
        .expect_err("a country silver cannot place");

    assert!(err.to_string().contains("FR"), "{err}");
}

#[tokio::test]
async fn a_dataset_partitioned_below_its_country_is_read_a_country_at_a_time() {
    const SESSIONS: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("session", "start_date");

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    for (country, at) in [
        (Country::Germany, BERLIN),
        (Country::UnitedKingdom, EDINBURGH),
    ] {
        root.dataset(SESSIONS)
            .partition(COUNTRY, country)
            .expect("a country partition")
            .partition("start_date", "2026-09-27")
            .expect("a date partition")
            .replace_with_geo(&[crossings_of(country, at)])
            .await
            .expect("write the country's rows");
    }

    let ids: Vec<Identified> =
        medallion::rows_of_every_country(&root, SESSIONS, "session", "SELECT id FROM session")
            .await
            .expect("read every country");

    assert_eq!(
        ids.iter().map(|row| row.id).sum::<i64>(),
        2,
        "one row from each country's date partition"
    );
}

#[derive(Debug, serde::Deserialize)]
struct Identified {
    id: i64,
}

#[tokio::test]
async fn each_country_is_read_from_its_own_partition() {
    let (_tmp, root) = store_of_two_countries().await;

    for country in Country::ALL {
        let query = Query::new(root.clone());
        let of_country = root
            .dataset(CROSSINGS)
            .partition(COUNTRY, country)
            .expect("a country partition");

        query
            .register_at(&of_country, CROSSINGS.name)
            .await
            .expect("register the country's rows");
        let count = query
            .count("SELECT count(*) AS count FROM water_crossing")
            .await
            .expect("count the rows");

        assert_eq!(count, 1, "{country}");
    }
}
