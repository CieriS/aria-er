use anyhow::{Context, Result};
use aq_core::{DateWindow, Sink, Source};
use chrono::NaiveDate;
use tracing::info;

/// Fetches the station registry and the measurements of `window` and stores them.
pub fn run<S: Source, K: Sink>(
    source: &S,
    sink: &K,
    window: DateWindow,
    extracted_on: NaiveDate,
) -> Result<()> {
    let sensors = source
        .fetch_stations()
        .context("fetching station registry")?;
    let station_rows = sink
        .write_stations(extracted_on, &sensors)
        .context("writing station registry")?;

    let measurements = source
        .fetch_measurements(window)
        .context("fetching measurements")?;
    let report = sink
        .write_measurements(&measurements)
        .context("writing measurements")?;

    info!(
        from = %window.from(),
        to = %window.to(),
        station_rows,
        fetched = measurements.len(),
        inserted = report.rows_inserted,
        updated = report.rows_updated,
        partitions_written = report.partitions_written,
        rows_stored = report.rows_stored,
        "ingestion completed"
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use aq_core::{Measurement, Pollutant, SourceError, Station, StationSensor};
    use aq_sink_parquet::ParquetSink;
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
        fn fetch_measurements(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError> {
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

        fn fetch_stations(&self) -> Result<Vec<StationSensor>, SourceError> {
            Ok(vec![StationSensor {
                station: Station {
                    id: 7_000_014,
                    name: "GIARDINI MARGHERITA".to_owned(),
                    municipality: "BOLOGNA".to_owned(),
                    province: "BO".to_owned(),
                    address: "VIALE BOTTONELLI".to_owned(),
                    altitude_m: Some(43.0),
                    longitude: None,
                    latitude: None,
                },
                pollutant: Pollutant {
                    id: 8,
                    name: "NO2".to_owned(),
                    unit: "ug/m3".to_owned(),
                },
            }])
        }
    }

    fn measurement(hour: u32, value: f64, flag: &str) -> Measurement {
        Measurement {
            station_id: 7_000_014,
            pollutant_id: 8,
            measured_at: Utc.with_ymd_and_hms(2026, 8, 5, hour, 0, 0).unwrap(),
            value,
            unit: Some("ug/m3".to_owned()),
            validation_flag: flag.to_owned(),
            raw_reftime: format!("08/05/2026 {:02}:00", hour + 1),
        }
    }

    #[test]
    fn rerun_with_revised_data_updates_in_place_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ParquetSink::new(dir.path().join("m"), dir.path().join("s"));
        let source = MockSource::new(vec![measurement(0, 10.0, "M"), measurement(1, 11.0, "M")]);
        let window = DateWindow::new(day(5), day(5)).unwrap();

        run(&source, &sink, window, day(6)).unwrap();
        // ARPAE validates the second hour and publishes a third one.
        *source.measurements.borrow_mut() = vec![
            measurement(0, 10.0, "M"),
            measurement(1, 12.0, "G"),
            measurement(2, 13.0, "M"),
        ];
        run(&source, &sink, window, day(6)).unwrap();

        let report = sink
            .write_measurements(&source.measurements.borrow())
            .unwrap();
        assert_eq!(report.rows_stored, 3);
        assert_eq!((report.rows_inserted, report.rows_updated), (0, 0));
        assert_eq!(*source.requested.borrow(), vec![window, window]);
        assert!(dir
            .path()
            .join("s/extracted_on=2026-08-06/stations.parquet")
            .exists());
    }

    #[test]
    fn source_failure_is_reported_with_context() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ParquetSink::new(dir.path().join("m"), dir.path().join("s"));
        let mut source = MockSource::new(Vec::new());
        source.fail = true;

        let error = run(
            &source,
            &sink,
            DateWindow::new(day(5), day(5)).unwrap(),
            day(6),
        )
        .unwrap_err();
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
}
