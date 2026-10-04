//! Parsing tests on reduced real samples taken from `data/samples/arpae/`.

use std::cell::RefCell;
use std::collections::HashMap;

use aq_core::{DateWindow, Source, SourceError};
use aq_http::{HttpConfig, Transport, TransportError};
use aq_source_arpae::parse::{
    parse_nrt_page, parse_pollutants_csv, parse_station_code, parse_stations_csv,
};
use aq_source_arpae::{ArpaeConfig, ArpaeSource};
use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};

const NRT_PAGE: &str = include_str!("fixtures/nrt_page.json");
const POLLUTANTS: &str = include_str!("fixtures/pollutants.csv");
const STATIONS: &str = include_str!("fixtures/stations.csv");
const EMPTY_PAGE: &str = r#"{"success": true, "result": {"records": [], "fields": []}}"#;

fn utc_plus_one() -> FixedOffset {
    FixedOffset::east_opt(3600).unwrap()
}

fn units() -> HashMap<u32, String> {
    parse_pollutants_csv(POLLUTANTS)
        .unwrap()
        .into_iter()
        .map(|p| (p.id, p.unit))
        .collect()
}

#[test]
fn pollutant_registry_keeps_published_units() {
    let pollutants = parse_pollutants_csv(POLLUTANTS).unwrap();
    assert_eq!(pollutants.len(), 21);
    let co = pollutants.iter().find(|p| p.id == 10).unwrap();
    assert_eq!(co.unit, "mg/m3");
    let pm10 = pollutants.iter().find(|p| p.id == 5).unwrap();
    assert_eq!((pm10.name.as_str(), pm10.unit.as_str()), ("PM10", "ug/m3"));
}

#[test]
fn station_registry_normalises_dotted_codes() {
    let sensors = parse_stations_csv(STATIONS).unwrap();
    assert_eq!(sensors.len(), 14);
    let first = &sensors[0];
    assert_eq!(first.station.id, 7_000_014);
    assert_eq!(first.station.name, "GIARDINI MARGHERITA");
    assert_eq!(first.station.municipality, "BOLOGNA");
    assert_eq!(first.station.altitude_m, Some(43.0));
    assert!((first.station.longitude.unwrap() - 11.354).abs() < 1e-3);
    assert_eq!(first.pollutant.id, 5);
    assert!(sensors.iter().any(|s| s.station.id == 2_000_003));
}

#[test]
fn station_code_accepts_the_three_published_forms() {
    for raw in ["7000014", "07000014", "7.000.014"] {
        assert_eq!(parse_station_code(raw), Some(7_000_014), "{raw}");
    }
    assert_eq!(parse_station_code("7,000"), None);
    assert_eq!(parse_station_code(""), None);
}

#[test]
fn nrt_page_is_parsed_month_first_and_converted_to_utc() {
    let page = parse_nrt_page(NRT_PAGE, &units(), utc_plus_one()).unwrap();
    assert_eq!(page.measurements.len(), 99);
    assert_eq!(page.last_id, Some(1098));

    // "08/05/2026 00:00" is 5 August (not 8 May) at UTC+1.
    let midnight = page
        .measurements
        .iter()
        .find(|m| m.pollutant_id == 8 && m.raw_reftime == "08/05/2026 00:00")
        .unwrap();
    assert_eq!(midnight.station_id, 7_000_014);
    assert_eq!(
        midnight.measured_at,
        Utc.with_ymd_and_hms(2026, 8, 4, 23, 0, 0).unwrap()
    );
    assert_eq!(midnight.unit.as_deref(), Some("ug/m3"));

    // Provisional and validated rows keep their flag.
    assert!(page.measurements.iter().any(|m| m.validation_flag == "M"));
    assert!(page.measurements.iter().any(|m| m.validation_flag == "G"));
}

#[test]
fn nrt_page_without_unit_registry_leaves_unit_empty() {
    let page = parse_nrt_page(NRT_PAGE, &HashMap::new(), utc_plus_one()).unwrap();
    assert!(page.measurements.iter().all(|m| m.unit.is_none()));
}

#[test]
fn malformed_records_fail_loudly() {
    let day_first = r#"{"success": true, "result": {"records": [
        {"_id": 1, "station_id": "7000014", "variable_id": "8",
         "reftime": "31/07/2026 00:00", "value": "5", "v_flag": "M"}]}}"#;
    let error = parse_nrt_page(day_first, &units(), utc_plus_one()).unwrap_err();
    assert!(error.to_string().contains("reftime"), "{error}");

    let bad_value = day_first
        .replace("31/07/2026", "07/31/2026")
        .replace("\"5\"", "\"n.d.\"");
    let error = parse_nrt_page(&bad_value, &units(), utc_plus_one()).unwrap_err();
    assert!(error.to_string().contains("value"), "{error}");

    let failed = r#"{"success": false, "error": {"message": "boom"}}"#;
    assert!(matches!(
        parse_nrt_page(failed, &units(), utc_plus_one()),
        Err(SourceError::Format { .. })
    ));
}

/// Serves the fixtures and records the SQL of each datastore query.
struct FixtureTransport {
    queries: RefCell<Vec<String>>,
    fail_first: RefCell<bool>,
}

impl Transport for FixtureTransport {
    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, TransportError> {
        match url {
            "http://sheets.test/pollutants" => Ok(POLLUTANTS.to_owned()),
            "http://sheets.test/stations" => Ok(STATIONS.to_owned()),
            "http://ckan.test/api/3/action/datastore_search_sql" => {
                if self.fail_first.replace(false) {
                    return Err(TransportError {
                        message: "HTTP status 502".to_owned(),
                        retryable: true,
                    });
                }
                let sql = query[0].1.to_owned();
                let first_page = sql.contains("_id > 0 ");
                self.queries.borrow_mut().push(sql);
                Ok(if first_page { NRT_PAGE } else { EMPTY_PAGE }.to_owned())
            }
            other => panic!("unexpected url {other}"),
        }
    }
}

fn source(transport: FixtureTransport) -> ArpaeSource<FixtureTransport> {
    let config = ArpaeConfig {
        ckan_base_url: "http://ckan.test/".to_owned(),
        measurements_resource_id: "4dc855a1-6298-4b71-a1ae-d80693d43dcb".to_owned(),
        stations_csv_url: "http://sheets.test/stations".to_owned(),
        pollutants_csv_url: "http://sheets.test/pollutants".to_owned(),
        utc_offset_hours: 1,
        page_size: 100,
    };
    let http = HttpConfig {
        connect_timeout_secs: 1,
        timeout_secs: 1,
        max_attempts: 2,
        initial_backoff_ms: 0,
    };
    ArpaeSource::new(transport, config, http).unwrap()
}

#[test]
fn source_pages_by_id_until_an_empty_page_and_retries() {
    let source = source(FixtureTransport {
        queries: RefCell::new(Vec::new()),
        fail_first: RefCell::new(true),
    });
    let window = DateWindow::new(
        NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 8, 5).unwrap(),
    )
    .unwrap();

    let measurements = source.fetch(window).unwrap();
    assert_eq!(measurements.len(), 99);
    assert!(measurements.iter().all(|m| m.unit.is_some()));

    let stations = source.stations().fetch(window).unwrap();
    assert_eq!(stations.len(), 14);
}

#[test]
fn source_queries_the_window_and_advances_the_cursor() {
    let transport = FixtureTransport {
        queries: RefCell::new(Vec::new()),
        fail_first: RefCell::new(false),
    };
    let window = DateWindow::new(
        NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 8, 5).unwrap(),
    )
    .unwrap();
    let source = source(transport);
    source.fetch(window).unwrap();

    let queries = source.transport().queries.borrow();
    assert_eq!(queries.len(), 2);
    assert!(queries[0].contains("BETWEEN '20260731' AND '20260805'"));
    assert!(queries[0].contains("LIMIT 100"));
    assert!(queries[1].contains("_id > 1098 "));
}

#[test]
fn source_rejects_a_resource_id_that_is_not_a_uuid() {
    let config = ArpaeConfig {
        ckan_base_url: "http://ckan.test".to_owned(),
        measurements_resource_id: "x\" OR 1=1 --".to_owned(),
        stations_csv_url: String::new(),
        pollutants_csv_url: String::new(),
        utc_offset_hours: 1,
        page_size: 100,
    };
    let http = HttpConfig {
        connect_timeout_secs: 1,
        timeout_secs: 1,
        max_attempts: 1,
        initial_backoff_ms: 0,
    };
    let transport = FixtureTransport {
        queries: RefCell::new(Vec::new()),
        fail_first: RefCell::new(false),
    };
    assert!(ArpaeSource::new(transport, config, http).is_err());
}
