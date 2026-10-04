use std::fs;

use aq_core::{Sink, WeatherObservation};
use aq_sink_parquet::WeatherSink;
use chrono::{TimeZone, Utc};
use tempfile::TempDir;

fn observation(location: &str, hour: u32, wind: Option<f64>) -> WeatherObservation {
    WeatherObservation {
        location_id: location.to_owned(),
        latitude: 44.5,
        longitude: 11.4,
        grid_latitude: 44.46,
        grid_longitude: 11.32,
        observed_at: Utc.with_ymd_and_hms(2025, 1, 1, hour, 0, 0).unwrap(),
        temperature_c: Some(1.7),
        precipitation_mm: Some(0.0),
        wind_speed_ms: wind,
        wind_direction_deg: None,
        surface_pressure_hpa: Some(1020.5),
    }
}

#[test]
fn weather_upsert_is_idempotent_and_keeps_nulls() {
    let dir = TempDir::new().unwrap();
    let sink = WeatherSink::new(dir.path());
    let rows = vec![
        observation("445_114", 1, None),
        observation("445_114", 0, Some(1.5)),
        observation("448_109", 0, Some(2.0)),
    ];
    let path = dir.path().join("year=2025/month=01/part-0.parquet");

    let first = sink.write(&rows).unwrap();
    assert_eq!((first.rows_inserted, first.rows_stored), (3, 3));
    let bytes = fs::read(&path).unwrap();

    // Same rows again: read back (nulls included), compared equal, file untouched.
    let second = sink.write(&rows).unwrap();
    assert_eq!((second.rows_inserted, second.rows_updated), (0, 0));
    assert_eq!(second.partitions_written, 0);
    assert_eq!(fs::read(&path).unwrap(), bytes);

    // A late value for the missing hour is an update, not a new row.
    let third = sink.write(&[observation("445_114", 1, Some(3.0))]).unwrap();
    assert_eq!((third.rows_inserted, third.rows_updated), (0, 1));
    assert_eq!(third.rows_stored, 3);
}
