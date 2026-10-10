//! The object store backend, exercised on `object_store`'s in-memory store.
#![cfg(feature = "gcs")]

use std::path::Path;
use std::sync::Arc;

use aq_core::{Measurement, Sink};
use aq_sink_parquet::{MeasurementSink, ObjectStorage, Storage};
use chrono::{TimeZone, Utc};
use object_store::memory::InMemory;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStoreExt;

fn measurement(hour: u32, value: f64) -> Measurement {
    Measurement {
        station_id: 7_000_014,
        pollutant_id: 8,
        measured_at: Utc.with_ymd_and_hms(2026, 8, 5, hour, 0, 0).unwrap(),
        value,
        unit: Some("ug/m3".to_owned()),
        validation_flag: Some("M".to_owned()),
        raw_reftime: format!("08/05/2026 {:02}:00", hour + 1),
    }
}

#[test]
fn missing_objects_read_as_none_and_writes_replace_the_object() {
    let storage = ObjectStorage::new(Arc::new(InMemory::new())).unwrap();
    let path = Path::new("raw/arpae/file.parquet");

    assert_eq!(storage.read(path).unwrap(), None);
    storage.write(path, b"first".to_vec()).unwrap();
    storage.write(path, b"second".to_vec()).unwrap();
    assert_eq!(storage.read(path).unwrap().as_deref(), Some(&b"second"[..]));
}

#[test]
fn upsert_on_an_object_store_matches_the_local_behaviour() {
    let store = Arc::new(InMemory::new());
    let storage = Arc::new(ObjectStorage::new(store.clone()).unwrap());
    let sink = MeasurementSink::with_storage(storage, "raw/arpae/measurements");

    let first = sink
        .write(&[measurement(0, 10.0), measurement(1, 11.0)])
        .unwrap();
    assert_eq!((first.rows_inserted, first.partitions_written), (2, 1));

    // Same rows again: nothing is rewritten.
    let again = sink
        .write(&[measurement(0, 10.0), measurement(1, 11.0)])
        .unwrap();
    assert_eq!((again.rows_inserted, again.rows_updated), (0, 0));
    assert_eq!(again.partitions_written, 0);

    // A revised value updates in place.
    let revised = sink.write(&[measurement(1, 12.5)]).unwrap();
    assert_eq!((revised.rows_updated, revised.rows_stored), (1, 2));

    // The object key is the partition path with `/` separators.
    let key = ObjectPath::from("raw/arpae/measurements/year=2026/month=08/part-0.parquet");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(runtime.block_on(store.head(&key)).is_ok());
}

#[test]
fn local_and_object_storage_produce_the_same_bytes() {
    let rows = [measurement(0, 10.0), measurement(1, 11.0)];

    let dir = tempfile::tempdir().unwrap();
    MeasurementSink::new(dir.path()).write(&rows).unwrap();
    let local = std::fs::read(dir.path().join("year=2026/month=08/part-0.parquet")).unwrap();

    let storage = Arc::new(ObjectStorage::new(Arc::new(InMemory::new())).unwrap());
    MeasurementSink::with_storage(storage.clone(), "m")
        .write(&rows)
        .unwrap();
    let remote = storage
        .read(Path::new("m/year=2026/month=08/part-0.parquet"))
        .unwrap()
        .unwrap();

    assert_eq!(local, remote);
}
