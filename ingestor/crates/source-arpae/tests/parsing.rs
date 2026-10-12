//! Parsing tests on reduced real samples taken from `data/samples/arpae/`.

use std::cell::RefCell;
use std::collections::BTreeMap;

use aq_core::{DateWindow, Source, SourceError};
use aq_http::{HttpConfig, Transport, TransportError};
use aq_source_arpae::parse::{
    parse_measurement_page, parse_station_code, parse_station_types, parse_stations_csv,
    MeasurementMapping,
};
use aq_source_arpae::{ArpaeConfig, ArpaeSource};
use chrono::{NaiveDate, TimeZone, Utc};

/// Porta San Felice (archive id 1103) on 2026-09-16: NO2, CO, PM10 and PM2.5 with
/// reference times from midnight to the midnight after, 54 records in two pages.
const MEASUREMENTS_PAGE_1: &str = include_str!("fixtures/measurements_page_1.json");
const MEASUREMENTS_PAGE_2: &str = include_str!("fixtures/measurements_page_2.json");
const STATIONS: &str = include_str!("fixtures/stations.csv");
const STATION_TYPES: &str = include_str!("fixtures/station_registry.json");

fn mapping() -> MeasurementMapping {
    MeasurementMapping {
        station_ids: [("1103".to_owned(), 7_000_015)].into(),
        pollutant_ids: [("32", 8), ("33", 10), ("48", 111), ("50", 5)]
            .map(|(archive, registry)| (archive.to_owned(), registry))
            .into(),
        daily_pollutant_ids: [5, 111].into(),
    }
}

fn day(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
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
fn measurement_page_maps_ids_and_keeps_published_values() {
    let page = parse_measurement_page(MEASUREMENTS_PAGE_1, &mapping()).unwrap();
    assert_eq!((page.records, page.total), (40, 54));
    assert_eq!(page.rows.len(), 40);
    assert!(page.unmapped_stations.is_empty());
    assert!(page
        .rows
        .iter()
        .all(|row| row.measurement.station_id == 7_000_015));

    let second = parse_measurement_page(MEASUREMENTS_PAGE_2, &mapping()).unwrap();
    assert_eq!((second.records, second.total), (14, 54));

    // CO keeps its own unit and the unrounded value; the validation stages are verbatim.
    let co = second
        .rows
        .iter()
        .map(|row| &row.measurement)
        .find(|m| m.raw_reftime == "2026-09-16T15:00:00+01:00")
        .unwrap();
    assert_eq!(co.pollutant_id, 10);
    assert_eq!(co.value, 0.45);
    assert_eq!(co.unit.as_deref(), Some("mg/m³"));
    assert_eq!(co.validation_flag.as_deref(), Some("Giornaliera + Mensile"));
}

#[test]
fn hourly_values_move_to_the_end_of_their_hour() {
    let page = parse_measurement_page(MEASUREMENTS_PAGE_1, &mapping()).unwrap();
    let first_hour = page
        .rows
        .iter()
        .find(|row| {
            row.measurement.pollutant_id == 8
                && row.measurement.raw_reftime == "2026-09-16T00:00:00+01:00"
        })
        .unwrap();

    // Published at the start of the hour, 00:00 UTC+1: the hour ends at 00:00 UTC.
    assert_eq!(first_hour.period_start_day, day(9, 16));
    assert_eq!(
        first_hour.measurement.measured_at,
        Utc.with_ymd_and_hms(2026, 9, 16, 0, 0, 0).unwrap()
    );
    assert_eq!(first_hour.measurement.value, 13.53);
}

#[test]
fn daily_values_move_from_the_end_of_their_day_to_the_day_itself() {
    let pages = [MEASUREMENTS_PAGE_1, MEASUREMENTS_PAGE_2]
        .map(|json| parse_measurement_page(json, &mapping()).unwrap());
    let pm10: Vec<_> = pages
        .iter()
        .flat_map(|page| &page.rows)
        .filter(|row| row.measurement.pollutant_id == 5)
        .collect();
    assert_eq!(pm10.len(), 2);

    // The value stamped at the midnight ending 15 September is the PM10 of that day
    // (the bulletin of 2026-09-15 reports 12 for this station).
    let stamped_16 = pm10
        .iter()
        .find(|row| row.measurement.raw_reftime == "2026-09-16T00:00:00+01:00")
        .unwrap();
    assert_eq!(stamped_16.period_start_day, day(9, 15));
    assert_eq!(stamped_16.measurement.value, 11.8);
    assert_eq!(
        stamped_16.measurement.measured_at,
        Utc.with_ymd_and_hms(2026, 9, 14, 23, 0, 0).unwrap()
    );

    let stamped_17 = pm10
        .iter()
        .find(|row| row.measurement.raw_reftime == "2026-09-17T00:00:00+01:00")
        .unwrap();
    assert_eq!(stamped_17.period_start_day, day(9, 16));
    assert_eq!(stamped_17.measurement.value, 11.2);
}

/// One archive record; `{}` placeholders are station id, parameter id, reference time and value.
fn page_with(station: &str, parameter: &str, reftime: &str, value: &str) -> String {
    format!(
        r#"{{"_items": [{{"_id": "x", "station_id": "{station}", "parameter_id": "{parameter}",
            "reftime": "{reftime}", "value": {value}, "parameter_unit": "µg/m³",
            "validity": "Non validato"}}], "_meta": {{"page": 1, "max_results": 1, "total": 1}}}}"#
    )
}

#[test]
fn records_of_an_unmapped_station_are_left_out_and_reported() {
    let json = page_with("2024", "32", "2026-09-16T00:00:00+01:00", "5.0");
    let page = parse_measurement_page(&json, &mapping()).unwrap();
    assert!(page.rows.is_empty());
    assert_eq!(page.records, 1);
    assert_eq!(
        page.unmapped_stations.into_iter().collect::<Vec<_>>(),
        ["2024"]
    );
}

#[test]
fn the_published_offset_decides_the_instant_and_the_day() {
    // The same instant written in UTC: the hour still ends at 00:00 UTC, and the day is
    // the one of the published offset.
    let json = page_with("1103", "32", "2026-09-15T23:00:00+00:00", "5.0");
    let page = parse_measurement_page(&json, &mapping()).unwrap();
    assert_eq!(page.rows[0].period_start_day, day(9, 15));
    assert_eq!(
        page.rows[0].measurement.measured_at,
        Utc.with_ymd_and_hms(2026, 9, 16, 0, 0, 0).unwrap()
    );
}

#[test]
fn malformed_records_fail_loudly() {
    let valid = page_with("1103", "32", "2026-09-16T00:00:00+01:00", "5.0");
    assert_eq!(
        parse_measurement_page(&valid, &mapping())
            .unwrap()
            .rows
            .len(),
        1
    );

    // A reference time without offset, as the old datastore wrote it.
    let error = parse_measurement_page(
        &page_with("1103", "32", "09/16/2026 00:00", "5.0"),
        &mapping(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("reftime"), "{error}");

    // A parameter that was not requested.
    let error = parse_measurement_page(
        &page_with("1103", "41", "2026-09-16T00:00:00+01:00", "5.0"),
        &mapping(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("parameter_id"), "{error}");

    // A value that is not a number, a missing field, a response that is not a page.
    for json in [
        page_with("1103", "32", "2026-09-16T00:00:00+01:00", "null"),
        page_with("1103", "32", "2026-09-16T00:00:00+01:00", "\"n.d.\""),
        valid.replace(r#""validity": "Non validato""#, r#""other": 1"#),
        r#"{"_error": {"code": 500, "message": "boom"}}"#.to_owned(),
    ] {
        assert!(
            matches!(
                parse_measurement_page(&json, &mapping()),
                Err(SourceError::Format { .. })
            ),
            "{json}"
        );
    }
}

/// Serves the fixtures and records the query of each request to the REST service.
struct FixtureTransport {
    queries: RefCell<Vec<Vec<(String, String)>>>,
    fail_first: RefCell<bool>,
}

impl FixtureTransport {
    fn new(fail_first: bool) -> Self {
        Self {
            queries: RefCell::new(Vec::new()),
            fail_first: RefCell::new(fail_first),
        }
    }

    fn record(&self, query: &[(&str, &str)]) {
        let query = query
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self.queries.borrow_mut().push(query);
    }
}

impl Transport for FixtureTransport {
    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, TransportError> {
        match url {
            "http://sheets.test/stations" => Ok(STATIONS.to_owned()),
            "http://apps.test/qa_archivio_dati_public" => {
                if self.fail_first.replace(false) {
                    return Err(TransportError {
                        message: "HTTP status 502".to_owned(),
                        retryable: true,
                    });
                }
                self.record(query);
                let page = query.iter().find(|(name, _)| *name == "page").unwrap().1;
                Ok(match page {
                    "1" => MEASUREMENTS_PAGE_1,
                    "2" => MEASUREMENTS_PAGE_2,
                    other => panic!("unexpected page {other}"),
                }
                .to_owned())
            }
            "http://apps.test/qa_stazioni" => {
                self.record(query);
                Ok(STATION_TYPES.to_owned())
            }
            other => panic!("unexpected url {other}"),
        }
    }
}

fn config() -> ArpaeConfig {
    let ids = |pairs: &[(&str, u32)]| -> BTreeMap<String, u32> {
        pairs
            .iter()
            .map(|(archive, registry)| ((*archive).to_owned(), *registry))
            .collect()
    };
    ArpaeConfig {
        measurements_url: "http://apps.test/qa_archivio_dati_public".to_owned(),
        stations_csv_url: "http://sheets.test/stations".to_owned(),
        station_types_url: "http://apps.test/qa_stazioni".to_owned(),
        archive_dir: String::new(),
        utc_offset_hours: 1,
        page_size: 40,
        daily_pollutant_ids: vec![5, 111],
        pollutant_ids: ids(&[("32", 8), ("33", 10), ("48", 111), ("50", 5)]),
        station_ids: ids(&[("1103", 7_000_015)]),
    }
}

fn http() -> HttpConfig {
    HttpConfig {
        connect_timeout_secs: 1,
        timeout_secs: 1,
        max_attempts: 2,
        initial_backoff_ms: 0,
    }
}

fn source(transport: FixtureTransport) -> ArpaeSource<FixtureTransport> {
    ArpaeSource::new(transport, config(), http()).unwrap()
}

#[test]
fn source_pages_until_the_reported_total_and_retries() {
    let source = source(FixtureTransport::new(true));
    let window = DateWindow::new(day(9, 16), day(9, 16)).unwrap();

    let measurements = source.fetch(window).unwrap();

    // Two pages, 54 records. Four belong to other days: the PM10 and PM2.5 stamped at
    // the midnight starting the day (15 September), the NO2 and CO of the hour starting
    // at the midnight after it (17 September).
    assert_eq!(source.transport().queries.borrow().len(), 2);
    assert_eq!(measurements.len(), 50);
    let count = |pollutant_id: u32| {
        measurements
            .iter()
            .filter(|m| m.pollutant_id == pollutant_id)
            .count()
    };
    assert_eq!((count(8), count(10), count(5), count(111)), (24, 24, 1, 1));
    assert!(measurements.iter().all(|m| m.unit.is_some()));

    let stations = source.stations().fetch(window).unwrap();
    assert_eq!(stations.len(), 14);
}

#[test]
fn source_queries_the_window_for_the_mapped_parameters() {
    let source = source(FixtureTransport::new(false));
    let window = DateWindow::new(day(9, 10), day(9, 16)).unwrap();
    source.fetch(window).unwrap();

    let queries = source.transport().queries.borrow();
    let param = |query: &[(String, String)], name: &str| {
        query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .unwrap()
    };
    let filter: serde_json::Value = serde_json::from_str(&param(&queries[0], "where")).unwrap();
    // From the midnight starting the first day to the one ending the last day.
    assert_eq!(filter["reftime"]["$gte"], "2026-09-10T00:00:00+01:00");
    assert_eq!(filter["reftime"]["$lte"], "2026-09-17T00:00:00+01:00");
    assert_eq!(
        filter["parameter_id"]["$in"],
        serde_json::json!(["32", "33", "48", "50"])
    );
    assert_eq!(param(&queries[0], "sort"), "_id");
    assert_eq!(param(&queries[0], "max_results"), "40");
    assert_eq!(
        (param(&queries[0], "page"), param(&queries[1], "page")),
        ("1".to_owned(), "2".to_owned())
    );
    assert_eq!(param(&queries[0], "where"), param(&queries[1], "where"));
}

#[test]
fn source_rejects_a_configuration_it_cannot_query_with() {
    let invalid = [
        ArpaeConfig {
            page_size: 0,
            ..config()
        },
        ArpaeConfig {
            utc_offset_hours: 99,
            ..config()
        },
        ArpaeConfig {
            pollutant_ids: BTreeMap::new(),
            ..config()
        },
        ArpaeConfig {
            station_ids: BTreeMap::new(),
            ..config()
        },
    ];
    for config in invalid {
        assert!(ArpaeSource::new(FixtureTransport::new(false), config, http()).is_err());
    }
}

#[test]
fn station_types_join_area_and_exposure_into_one_label() {
    let types = parse_station_types(STATION_TYPES).unwrap();
    assert_eq!(types.len(), 6);

    let by_id = |id: u32| types.iter().find(|t| t.station_id == id).unwrap();
    let porta_san_felice = by_id(7_000_015);
    assert_eq!(porta_san_felice.station_name, "PORTA SAN FELICE");
    assert_eq!(porta_san_felice.province, "BO");
    assert_eq!(porta_san_felice.type_label, "Urbana Traffico");
    assert_eq!(by_id(7_000_041).type_label, "Suburbana Fondo");
    assert_eq!(by_id(2_000_229).type_label, "Rurale Industriale");
    // A mobile laboratory has neither area nor exposure.
    assert_eq!(by_id(2_000_224).type_label, "");

    // The version is the day the registry record was last updated.
    assert_eq!(porta_san_felice.bulletin_id.len(), 8);
    assert!(porta_san_felice.bulletin_id.starts_with("2026"));
}

#[test]
fn incomplete_or_malformed_registry_fails_loudly() {
    let station = r#"{"_id": "7000015", "nome": "PORTA SAN FELICE", "sigla_provincia": "BO",
        "tipo_stazione": "Traffico", "zona": "Urbana",
        "_updated": "Sat, 10 Oct 2026 05:40:51 GMT"}"#;
    let page = |items: &str, total: u32| {
        format!(r#"{{"_items": [{items}], "_meta": {{"total": {total}}}}}"#)
    };
    assert_eq!(parse_station_types(&page(station, 1)).unwrap().len(), 1);

    // More stations exist than the page returned.
    let error = parse_station_types(&page(station, 2)).unwrap_err();
    assert!(
        error.to_string().contains("no longer fits one page"),
        "{error}"
    );
    // No station at all.
    assert!(parse_station_types(&page("", 0)).is_err());
    // A code or a date that cannot be read.
    assert!(parse_station_types(&page(&station.replace("7000015", "x1"), 1)).is_err());
    assert!(parse_station_types(&page(
        &station.replace("Sat, 10 Oct 2026 05:40:51 GMT", "today"),
        1
    ))
    .is_err());
    // The exposure field is gone (as happened to the bulletin label).
    assert!(parse_station_types(&page(
        &station.replace(r#""tipo_stazione": "Traffico","#, ""),
        1
    ))
    .is_err());
}

#[test]
fn source_asks_the_registry_for_every_station_without_photos() {
    let source = source(FixtureTransport::new(false));

    let types = source
        .station_types()
        .fetch(DateWindow::new(day(10, 10), day(10, 10)).unwrap())
        .unwrap();

    assert_eq!(types.len(), 6);
    let queries = source.transport().queries.borrow();
    assert_eq!(queries.len(), 1);
    let has = |name: &str, value: &str| queries[0].contains(&(name.to_owned(), value.to_owned()));
    assert!(has("max_results", "500"), "{:?}", queries[0]);
    assert!(
        has("projection", r#"{"foto":0,"dati":0}"#),
        "{:?}",
        queries[0]
    );
}
