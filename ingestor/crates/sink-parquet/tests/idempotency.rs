use std::fs;
use std::path::{Path, PathBuf};

use aq_core::{Measurement, Pollutant, Sink, Station, StationSensor};
use aq_sink_parquet::ParquetSink;
use arrow_array::cast::AsArray;
use arrow_array::types::Float64Type;
use chrono::{NaiveDate, TimeZone, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use tempfile::TempDir;

fn measurement(day: u32, hour: u32, month: u32, value: f64, flag: &str) -> Measurement {
    Measurement {
        station_id: 7_000_014,
        pollutant_id: 8,
        measured_at: Utc.with_ymd_and_hms(2026, month, day, hour, 0, 0).unwrap(),
        value,
        unit: Some("ug/m3".to_owned()),
        validation_flag: flag.to_owned(),
        raw_reftime: format!("{month:02}/{day:02}/2026 {:02}:00", hour + 1),
    }
}

fn sink(dir: &TempDir) -> ParquetSink {
    ParquetSink::new(dir.path().join("measurements"), dir.path().join("stations"))
}

fn partition(dir: &TempDir, month: u32) -> PathBuf {
    dir.path().join(format!(
        "measurements/year=2026/month={month:02}/part-0.parquet"
    ))
}

fn values(path: &Path) -> Vec<f64> {
    let file = fs::File::open(path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    reader
        .flat_map(|batch| {
            let batch = batch.unwrap();
            let column = batch.column_by_name("value").unwrap();
            column.as_primitive::<Float64Type>().values().to_vec()
        })
        .collect()
}

#[test]
fn writes_one_file_per_utc_month() {
    let dir = TempDir::new().unwrap();
    let rows = vec![
        measurement(31, 22, 7, 5.0, "G"),
        measurement(1, 0, 8, 6.0, "M"),
        measurement(1, 1, 8, 7.0, "M"),
    ];
    let report = sink(&dir).write_measurements(&rows).unwrap();

    assert_eq!(report.partitions_written, 2);
    assert_eq!(report.rows_inserted, 3);
    assert_eq!(values(&partition(&dir, 7)), vec![5.0]);
    assert_eq!(values(&partition(&dir, 8)), vec![6.0, 7.0]);
}

#[test]
fn rerunning_the_same_window_is_byte_identical_and_adds_no_rows() {
    let dir = TempDir::new().unwrap();
    let rows: Vec<_> = (0..24)
        .map(|h| measurement(5, h, 8, f64::from(h), "M"))
        .collect();
    let sink = sink(&dir);

    sink.write_measurements(&rows).unwrap();
    let first = fs::read(partition(&dir, 8)).unwrap();

    let report = sink.write_measurements(&rows).unwrap();
    assert_eq!(report.rows_stored, 24);
    assert_eq!((report.rows_inserted, report.rows_updated), (0, 0));
    assert_eq!(report.partitions_written, 0);
    assert_eq!(fs::read(partition(&dir, 8)).unwrap(), first);
}

#[test]
fn output_does_not_depend_on_input_order_or_batching() {
    let rows: Vec<_> = (0..24)
        .map(|h| measurement(5, h, 8, f64::from(h), "M"))
        .collect();

    let whole = TempDir::new().unwrap();
    sink(&whole).write_measurements(&rows).unwrap();

    let split = TempDir::new().unwrap();
    let mut reversed = rows.clone();
    reversed.reverse();
    let split_sink = sink(&split);
    split_sink.write_measurements(&reversed[..10]).unwrap();
    split_sink.write_measurements(&reversed[10..]).unwrap();

    assert_eq!(
        fs::read(partition(&whole, 8)).unwrap(),
        fs::read(partition(&split, 8)).unwrap()
    );
}

#[test]
fn revised_value_replaces_the_provisional_one_and_keeps_other_rows() {
    let dir = TempDir::new().unwrap();
    let sink = sink(&dir);
    sink.write_measurements(&[
        measurement(5, 0, 8, 10.0, "M"),
        measurement(5, 1, 8, 11.0, "M"),
    ])
    .unwrap();

    let report = sink
        .write_measurements(&[measurement(5, 1, 8, 12.5, "G")])
        .unwrap();

    assert_eq!((report.rows_inserted, report.rows_updated), (0, 1));
    assert_eq!(report.rows_stored, 2);
    assert_eq!(values(&partition(&dir, 8)), vec![10.0, 12.5]);
}

#[test]
fn duplicate_keys_in_one_batch_collapse_to_the_last_row() {
    let dir = TempDir::new().unwrap();
    sink(&dir)
        .write_measurements(&[
            measurement(5, 0, 8, 1.0, "M"),
            measurement(5, 0, 8, 2.0, "M"),
        ])
        .unwrap();
    assert_eq!(values(&partition(&dir, 8)), vec![2.0]);
}

#[test]
fn no_temporary_file_is_left_behind() {
    let dir = TempDir::new().unwrap();
    sink(&dir)
        .write_measurements(&[measurement(5, 0, 8, 1.0, "M")])
        .unwrap();
    let names: Vec<_> = fs::read_dir(partition(&dir, 8).parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["part-0.parquet"]);
}

#[test]
fn station_snapshot_is_dated_and_overwritten_on_the_same_day() {
    let dir = TempDir::new().unwrap();
    let sensor = |pollutant_id: u32| StationSensor {
        station: Station {
            id: 7_000_014,
            name: "GIARDINI MARGHERITA".to_owned(),
            municipality: "BOLOGNA".to_owned(),
            province: "BO".to_owned(),
            address: "VIALE BOTTONELLI".to_owned(),
            altitude_m: Some(43.0),
            longitude: Some(11.354),
            latitude: None,
        },
        pollutant: Pollutant {
            id: pollutant_id,
            name: "PM10".to_owned(),
            unit: "ug/m3".to_owned(),
        },
    };
    let day = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let sink = sink(&dir);
    let path = dir
        .path()
        .join("stations/extracted_on=2026-10-01/stations.parquet");

    assert_eq!(
        sink.write_stations(day, &[sensor(8), sensor(5)]).unwrap(),
        2
    );
    let first = fs::read(&path).unwrap();
    assert_eq!(
        sink.write_stations(day, &[sensor(5), sensor(8)]).unwrap(),
        2
    );
    assert_eq!(fs::read(&path).unwrap(), first);
}
