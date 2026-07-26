//! Writing batches into the store: directories are created, the file is readable back as
//! parquet, and an interrupted write leaves nothing behind.

use std::sync::Arc;

use arrow::array::{Int64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::{TimeZone, Utc};
use medallion::{write_batches, Layer, Root, WriteError};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn batch(ids: Vec<i64>, names: Vec<&str>) -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
    ]));
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(names)),
        ],
    )
    .unwrap()
}

#[test]
fn writing_creates_the_partition_directories_and_a_readable_parquet_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = Root::new(tmp.path())
        .dataset(Layer::Bronze, "sensor_reading")
        .partition("sensor", "gps")
        .unwrap()
        .batch_file(Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap());

    write_batches(&path, &[batch(vec![1, 2], vec!["a", "b"])]).unwrap();

    assert!(path.exists(), "{} should exist", path.display());
    let file = std::fs::File::open(&path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    let rows: usize = reader.map(|b| b.unwrap().num_rows()).sum();
    assert_eq!(rows, 2);
}

#[test]
fn multiple_batches_are_written_into_the_one_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = Root::new(tmp.path())
        .dataset(Layer::Bronze, "sensor_reading")
        .file("part-0");

    write_batches(
        &path,
        &[batch(vec![1], vec!["a"]), batch(vec![2, 3], vec!["b", "c"])],
    )
    .unwrap();

    let file = std::fs::File::open(&path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    let rows: usize = reader.map(|b| b.unwrap().num_rows()).sum();
    assert_eq!(rows, 3);
}

#[test]
fn writing_no_batches_is_an_error_rather_than_an_empty_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = Root::new(tmp.path())
        .dataset(Layer::Bronze, "sensor_reading")
        .file("part-0");

    let err = write_batches(&path, &[]).unwrap_err();

    assert!(matches!(err, WriteError::Empty), "unexpected error: {err}");
    assert!(!path.exists(), "no file should have been created");
}

#[test]
fn a_failed_write_leaves_no_file_at_the_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let path = Root::new(tmp.path())
        .dataset(Layer::Bronze, "sensor_reading")
        .file("part-0");
    // Batches with differing schemas: the first is accepted, the second fails mid-write.
    let mismatched = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![9]))],
    )
    .unwrap();

    let err = write_batches(&path, &[batch(vec![1], vec!["a"]), mismatched]).unwrap_err();

    assert!(matches!(err, WriteError::Parquet(_)), "unexpected: {err}");
    assert!(
        !path.exists(),
        "a partial file was left at {}",
        path.display()
    );
    let staged: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(staged.is_empty(), "staging files left behind: {staged:?}");
}
