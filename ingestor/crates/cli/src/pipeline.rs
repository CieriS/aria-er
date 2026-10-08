use anyhow::{Context, Result};
use aq_core::{DateWindow, Sink, Source, StationSensor, WriteReport};
use aq_source_openmeteo::{dedup_locations, Location};
use chrono::NaiveDate;

/// Outcome of moving one kind of record from a source to a sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ingested {
    pub fetched: usize,
    pub report: WriteReport,
}

/// Fetches the records of `window` from `source` and stores them in `sink`.
///
/// `what` names the records in error messages.
pub fn ingest<S, K>(source: &S, sink: &K, window: DateWindow, what: &str) -> Result<Ingested>
where
    S: Source,
    K: Sink<Record = S::Record>,
{
    let records = source
        .fetch(window)
        .with_context(|| format!("fetching {what}"))?;
    let report = sink
        .write(&records)
        .with_context(|| format!("writing {what}"))?;
    Ok(Ingested {
        fetched: records.len(),
        report,
    })
}

/// Window to ingest: explicit bounds win, otherwise the reprocessing window ending today.
pub fn resolve_window(
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    today: NaiveDate,
    reprocess_window_days: u32,
) -> Result<DateWindow> {
    let to = to.unwrap_or(today);
    match from {
        Some(from) => {
            DateWindow::new(from, to).with_context(|| format!("--from {from} is after --to {to}"))
        }
        None => Ok(DateWindow::ending_on(to, reprocess_window_days)),
    }
}

/// The part of calendar year `year` inside the optional bounds, if any.
pub fn year_window(
    year: i32,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Option<DateWindow> {
    let first = NaiveDate::from_ymd_opt(year, 1, 1)?;
    let last = NaiveDate::from_ymd_opt(year, 12, 31)?;
    DateWindow::new(
        from.map_or(first, |f| f.max(first)),
        to.map_or(last, |t| t.min(last)),
    )
}

/// Weather locations covering the stations of the registry that have coordinates.
pub fn weather_locations(sensors: &[StationSensor], coordinate_decimals: u32) -> Vec<Location> {
    let coordinates = sensors
        .iter()
        .filter_map(|s| Some((s.station.latitude?, s.station.longitude?)));
    dedup_locations(coordinates, coordinate_decimals)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use aq_core::{Measurement, Pollutant, SourceError, Station};
    use aq_sink_parquet::MeasurementSink;
    use chrono::{TimeZone, Utc};

    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, d).unwrap()
    }

    /// In-memory source: returns whatever it currently holds.
    struct MockSource {
        measurements: RefCell<Vec<Measurement>>,
        requested: RefCell<Vec<DateWindow>>,
        fail: bool,
    }

    impl MockSource {
        fn new(measurements: Vec<Measurement>) -> Self {
            Self {
                measurements: RefCell::new(measurements),
                requested: RefCell::new(Vec::new()),
                fail: false,
            }
        }
    }

    impl Source for MockSource {
        type Record = Measurement;

        fn fetch(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError> {
            self.requested.borrow_mut().push(window);
            if self.fail {
                return Err(SourceError::Transport {
                    url: "mock".to_owned(),
                    attempts: 1,
                    message: "down".to_owned(),
                });
            }
            Ok(self.measurements.borrow().clone())
        }
    }

    fn measurement(hour: u32, value: f64, flag: &str) -> Measurement {
        Measurement {
            station_id: 7_000_014,
            pollutant_id: 8,
            measured_at: Utc.with_ymd_and_hms(2026, 8, 5, hour, 0, 0).unwrap(),
            value,
            unit: Some("ug/m3".to_owned()),
            validation_flag: Some(flag.to_owned()),
            raw_reftime: format!("08/05/2026 {:02}:00", hour + 1),
        }
    }

    fn sensor(id: u32, coordinates: Option<(f64, f64)>) -> StationSensor {
        StationSensor {
            station: Station {
                id,
                name: "STATION".to_owned(),
                municipality: "BOLOGNA".to_owned(),
                province: "BO".to_owned(),
                address: String::new(),
                altitude_m: None,
                latitude: coordinates.map(|c| c.0),
                longitude: coordinates.map(|c| c.1),
            },
            pollutant: Pollutant {
                id: 5,
                name: "PM10".to_owned(),
                unit: "ug/m3".to_owned(),
            },
        }
    }

    #[test]
    fn rerun_with_revised_data_updates_in_place_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let sink = MeasurementSink::new(dir.path());
        let source = MockSource::new(vec![measurement(0, 10.0, "M"), measurement(1, 11.0, "M")]);
        let window = DateWindow::new(day(5), day(5)).unwrap();

        let first = ingest(&source, &sink, window, "measurements").unwrap();
        assert_eq!((first.fetched, first.report.rows_inserted), (2, 2));

        // ARPAE validates the second hour and publishes a third one.
        *source.measurements.borrow_mut() = vec![
            measurement(0, 10.0, "M"),
            measurement(1, 12.0, "G"),
            measurement(2, 13.0, "M"),
        ];
        let second = ingest(&source, &sink, window, "measurements").unwrap();
        assert_eq!(second.fetched, 3);
        assert_eq!(
            (second.report.rows_inserted, second.report.rows_updated),
            (1, 1)
        );
        assert_eq!(second.report.rows_stored, 3);
        assert_eq!(*source.requested.borrow(), vec![window, window]);
    }

    #[test]
    fn source_failure_is_reported_with_context_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let sink = MeasurementSink::new(dir.path().join("m"));
        let mut source = MockSource::new(Vec::new());
        source.fail = true;
        let window = DateWindow::new(day(5), day(5)).unwrap();

        let error = ingest(&source, &sink, window, "measurements").unwrap_err();
        assert!(format!("{error:#}").contains("fetching measurements"));
        assert!(!dir.path().join("m").exists());
    }

    #[test]
    fn default_window_is_the_reprocessing_window_ending_today() {
        let window = resolve_window(None, None, day(31), 30).unwrap();
        assert_eq!((window.from(), window.to()), (day(2), day(31)));
    }

    #[test]
    fn explicit_bounds_override_the_default_window() {
        let window = resolve_window(Some(day(1)), Some(day(10)), day(31), 30).unwrap();
        assert_eq!((window.from(), window.to()), (day(1), day(10)));

        let window = resolve_window(None, Some(day(10)), day(31), 5).unwrap();
        assert_eq!((window.from(), window.to()), (day(6), day(10)));

        assert!(resolve_window(Some(day(10)), Some(day(1)), day(31), 30).is_err());
    }

    #[test]
    fn year_window_clips_a_year_to_the_requested_bounds() {
        let full = year_window(2025, None, None).unwrap();
        assert_eq!(full.from(), NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        assert_eq!(full.to(), NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());

        let clipped = year_window(2026, Some(day(5)), Some(day(10))).unwrap();
        assert_eq!((clipped.from(), clipped.to()), (day(5), day(10)));

        // Years entirely outside the bounds are skipped.
        assert!(year_window(2025, Some(day(5)), None).is_none());
        assert!(year_window(2027, None, Some(day(10))).is_none());
    }

    #[test]
    fn weather_locations_merge_nearby_stations_and_skip_missing_coordinates() {
        let sensors = [
            sensor(7_000_014, Some((44.4827, 11.3541))),
            sensor(7_000_099, Some((44.4712, 11.3893))),
            sensor(2_000_003, Some((44.7937, 10.3306))),
            sensor(9_999_999, None),
        ];
        let ids: Vec<_> = weather_locations(&sensors, 1)
            .into_iter()
            .map(|l| l.id)
            .collect();
        assert_eq!(ids, ["445_114", "448_103"]);
    }
}
