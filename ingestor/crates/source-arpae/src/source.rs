use std::collections::BTreeSet;

use aq_core::{DateWindow, Measurement, Source, SourceError, StationSensor, StationType};
use aq_http::{get_with_retry, HttpConfig, Transport};
use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone};
use tracing::{debug, info, warn};

use crate::parse::{
    parse_measurement_page, parse_station_types, parse_stations_csv, MeasurementMapping,
};
use crate::ArpaeConfig;

/// ARPAE source backed by the REST measurement archive and the station registries.
pub struct ArpaeSource<T> {
    transport: T,
    config: ArpaeConfig,
    http: HttpConfig,
    offset: FixedOffset,
    mapping: MeasurementMapping,
}

impl<T: Transport> ArpaeSource<T> {
    pub fn new(transport: T, config: ArpaeConfig, http: HttpConfig) -> Result<Self, SourceError> {
        let config_error = |message: String| SourceError::Format {
            context: "ARPAE configuration".to_owned(),
            message,
        };
        let offset = config
            .utc_offset_hours
            .checked_mul(3600)
            .and_then(FixedOffset::east_opt)
            .ok_or_else(|| config_error(format!("utc_offset_hours {}", config.utc_offset_hours)))?;
        if config.page_size == 0 {
            return Err(config_error("page_size must be positive".to_owned()));
        }
        if config.pollutant_ids.is_empty() || config.station_ids.is_empty() {
            return Err(config_error(
                "pollutant_ids and station_ids must list at least one id".to_owned(),
            ));
        }
        let mapping = MeasurementMapping {
            station_ids: config.station_ids.clone().into_iter().collect(),
            pollutant_ids: config.pollutant_ids.clone().into_iter().collect(),
            daily_pollutant_ids: config.daily_pollutant_ids.iter().copied().collect(),
        };
        Ok(Self {
            transport,
            config,
            http,
            offset,
            mapping,
        })
    }

    /// The underlying transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The station registry published next to the measurements.
    pub fn stations(&self) -> ArpaeStations<'_, T> {
        ArpaeStations(self)
    }

    /// The type of each station, from the ARPAE station registry.
    pub fn station_types(&self) -> ArpaeStationTypes<'_, T> {
        ArpaeStationTypes(self)
    }

    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, SourceError> {
        get_with_retry(&self.transport, url, query, &self.http)
    }

    /// Midnight starting `day`, as the archive writes it.
    fn midnight(&self, day: NaiveDate) -> Result<String, SourceError> {
        self.offset
            .from_local_datetime(&day.and_time(NaiveTime::MIN))
            .single()
            .map(|time| time.to_rfc3339())
            .ok_or_else(|| SourceError::Format {
                context: "ARPAE window".to_owned(),
                message: format!("day {day}"),
            })
    }

    /// Filter on the published reference times that can belong to `window`, for the
    /// mapped parameters. A daily value is stamped at the end of its day, so the last
    /// day of the window ends at the midnight after it.
    fn measurement_filter(&self, window: DateWindow) -> Result<String, SourceError> {
        let after = window.to().succ_opt().unwrap_or(window.to());
        let parameters: Vec<&str> = self
            .config
            .pollutant_ids
            .keys()
            .map(String::as_str)
            .collect();
        let filter = serde_json::json!({
            "reftime": {
                "$gte": self.midnight(window.from())?,
                "$lte": self.midnight(after)?,
            },
            "parameter_id": { "$in": parameters },
        });
        Ok(filter.to_string())
    }
}

impl<T: Transport> Source for ArpaeSource<T> {
    type Record = Measurement;

    /// Measurements whose averaging period starts on a day of `window`.
    fn fetch(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError> {
        let filter = self.measurement_filter(window)?;
        let page_size = self.config.page_size.to_string();

        let mut measurements = Vec::new();
        let mut unmapped_stations = BTreeSet::new();
        let mut records = 0;
        // The server may cap a page below `page_size`: the scan ends on the total it reports.
        for page_number in 1u32.. {
            let page_number = page_number.to_string();
            let body = self.get(
                &self.config.measurements_url,
                &[
                    ("where", &filter),
                    ("sort", "_id"),
                    ("max_results", &page_size),
                    ("page", &page_number),
                ],
            )?;
            let page = parse_measurement_page(&body, &self.mapping)?;
            debug!(page = %page_number, records = page.records, total = page.total, "fetched page");
            records += page.records;
            unmapped_stations.extend(page.unmapped_stations);
            measurements.extend(
                page.rows
                    .into_iter()
                    .filter(|row| window.contains(row.period_start_day))
                    .map(|row| row.measurement),
            );
            if page.records == 0 || records >= page.total {
                break;
            }
        }

        if !unmapped_stations.is_empty() {
            warn!(
                stations = ?unmapped_stations,
                "measurements of stations missing from arpae.station_ids were left out"
            );
        }
        info!(
            from = %window.from(),
            to = %window.to(),
            rows = measurements.len(),
            "fetched ARPAE measurements"
        );
        Ok(measurements)
    }
}

/// The station registry of an [`ArpaeSource`], as a source of its own.
pub struct ArpaeStations<'a, T>(&'a ArpaeSource<T>);

impl<T: Transport> Source for ArpaeStations<'_, T> {
    type Record = StationSensor;

    /// The registry is a current snapshot: the window is ignored.
    fn fetch(&self, _window: DateWindow) -> Result<Vec<StationSensor>, SourceError> {
        let csv_text = self.0.get(&self.0.config.stations_csv_url, &[])?;
        let sensors = parse_stations_csv(&csv_text)?;
        info!(rows = sensors.len(), "fetched ARPAE station registry");
        Ok(sensors)
    }
}

/// Largest registry requested in one call; the registry has under a hundred stations.
const STATION_TYPES_PAGE_SIZE: &str = "500";
/// Fields left out of the response: a photo of each station and its recent bulletins.
const STATION_TYPES_PROJECTION: &str = r#"{"foto":0,"dati":0}"#;

/// The station types of the registry behind an [`ArpaeSource`].
pub struct ArpaeStationTypes<'a, T>(&'a ArpaeSource<T>);

impl<T: Transport> Source for ArpaeStationTypes<'_, T> {
    type Record = StationType;

    /// The registry is a current snapshot: the window is ignored.
    fn fetch(&self, _window: DateWindow) -> Result<Vec<StationType>, SourceError> {
        let body = self.0.get(
            &self.0.config.station_types_url,
            &[
                ("t", "json"),
                ("max_results", STATION_TYPES_PAGE_SIZE),
                ("projection", STATION_TYPES_PROJECTION),
            ],
        )?;
        let types = parse_station_types(&body)?;
        info!(rows = types.len(), "fetched ARPAE station types");
        Ok(types)
    }
}
