//! Parsing tests on real archive responses (Bologna and Modena, 23 February 2025).

use std::cell::RefCell;

use aq_core::{DateWindow, Source};
use aq_http::{HttpConfig, Transport, TransportError};
use aq_source_openmeteo::parse::parse_archive;
use aq_source_openmeteo::{dedup_locations, Location, OpenMeteoConfig, OpenMeteoSource};
use chrono::{NaiveDate, TimeZone, Utc};

const ONE: &str = include_str!("fixtures/archive_one_location.json");
const TWO: &str = include_str!("fixtures/archive_two_locations.json");

fn locations() -> Vec<Location> {
    dedup_locations([(44.5, 11.4), (44.8, 10.9)], 1)
}

#[test]
fn single_location_response_is_an_object() {
    let rows = parse_archive(ONE, &locations()[..1]).unwrap();
    assert_eq!(rows.len(), 24);

    let first = &rows[0];
    assert_eq!(first.location_id, "445_114");
    assert_eq!(
        first.observed_at,
        Utc.with_ymd_and_hms(2025, 2, 23, 0, 0, 0).unwrap()
    );
    assert_eq!((first.latitude, first.longitude), (44.5, 11.4));
    // The provider answers with its own grid cell, close to the requested point.
    assert!((first.grid_latitude - 44.5).abs() < 0.1);
    assert!(first.temperature_c.is_some() && first.wind_speed_ms.is_some());
    assert!(first.surface_pressure_hpa.unwrap() > 900.0);
}

#[test]
fn multi_location_response_keeps_request_order() {
    let rows = parse_archive(TWO, &locations()).unwrap();
    assert_eq!(rows.len(), 48);
    assert!(rows[..24].iter().all(|r| r.location_id == "445_114"));
    assert!(rows[24..].iter().all(|r| r.location_id == "448_109"));
}

#[test]
fn hours_without_any_value_are_skipped_and_partial_ones_kept() {
    let json = r#"{"latitude": 44.5, "longitude": 11.4, "utc_offset_seconds": 0, "hourly": {
        "time": ["2026-10-03T22:00", "2026-10-03T23:00"],
        "temperature_2m": [12.5, null], "precipitation": [null, null],
        "wind_speed_10m": [null, null], "wind_direction_10m": [null, null],
        "surface_pressure": [null, null]}}"#;
    let rows = parse_archive(json, &locations()[..1]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].temperature_c, Some(12.5));
    assert_eq!(rows[0].precipitation_mm, None);
}

#[test]
fn unexpected_responses_fail_loudly() {
    // Fewer locations than requested.
    assert!(parse_archive(ONE, &locations()).is_err());
    // API error body.
    assert!(parse_archive(
        r#"{"reason":"Bad Request","error":true}"#,
        &locations()[..1]
    )
    .is_err());
    // Timestamps not in UTC.
    let shifted = ONE.replace("\"utc_offset_seconds\": 0", "\"utc_offset_seconds\": 3600");
    assert!(parse_archive(&shifted, &locations()[..1]).is_err());
}

struct FixtureTransport {
    queries: RefCell<Vec<Vec<(String, String)>>>,
}

impl Transport for FixtureTransport {
    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, TransportError> {
        assert_eq!(url, "http://openmeteo.test/v1/archive");
        self.queries.borrow_mut().push(
            query
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        Ok(ONE.to_owned())
    }
}

#[test]
fn source_chunks_locations_and_queries_utc_days() {
    let config = OpenMeteoConfig {
        archive_url: "http://openmeteo.test/v1/archive".to_owned(),
        coordinate_decimals: 1,
        max_locations_per_request: 1,
    };
    let http = HttpConfig {
        connect_timeout_secs: 1,
        timeout_secs: 1,
        max_attempts: 1,
        initial_backoff_ms: 0,
    };
    let transport = FixtureTransport {
        queries: RefCell::new(Vec::new()),
    };
    let source = OpenMeteoSource::new(transport, config, http, locations()).unwrap();
    let day = NaiveDate::from_ymd_opt(2025, 2, 23).unwrap();

    let rows = source.fetch(DateWindow::new(day, day).unwrap()).unwrap();

    // One request per location, 24 hours each.
    assert_eq!(rows.len(), 48);
    let queries = source.transport().queries.borrow();
    assert_eq!(queries.len(), 2);
    let param = |i: usize, name: &str| {
        queries[i]
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert_eq!(param(0, "latitude"), "44.5");
    assert_eq!(param(1, "longitude"), "10.9");
    assert_eq!(param(0, "start_date"), "2025-02-23");
    assert_eq!(param(0, "timezone"), "GMT");
    assert_eq!(param(0, "wind_speed_unit"), "ms");
}
