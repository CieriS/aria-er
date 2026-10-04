use std::collections::HashMap;

use aq_core::{DateWindow, Measurement, Source, SourceError, StationSensor};
use aq_http::{get_with_retry, HttpConfig, Transport};
use chrono::FixedOffset;
use tracing::{debug, info, warn};

use crate::parse::{parse_nrt_page, parse_pollutants_csv, parse_stations_csv};
use crate::ArpaeConfig;

/// ARPAE source backed by the CKAN datastore and the registry CSV exports.
pub struct ArpaeSource<T> {
    transport: T,
    config: ArpaeConfig,
    http: HttpConfig,
    offset: FixedOffset,
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
        // The id is interpolated into SQL: accept only the characters of a UUID.
        let id = &config.measurements_resource_id;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err(config_error(format!("measurements_resource_id {id:?}")));
        }
        if config.page_size == 0 {
            return Err(config_error("page_size must be positive".to_owned()));
        }
        Ok(Self {
            transport,
            config,
            http,
            offset,
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

    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, SourceError> {
        get_with_retry(&self.transport, url, query, &self.http)
    }

    fn fetch_units(&self) -> Result<HashMap<u32, String>, SourceError> {
        let csv_text = self.get(&self.config.pollutants_csv_url, &[])?;
        Ok(parse_pollutants_csv(&csv_text)?
            .into_iter()
            .map(|p| (p.id, p.unit))
            .collect())
    }

    /// `reftime` is text formatted `MM/DD/YYYY HH:MM`: rebuild `YYYYMMDD` to
    /// compare days. Rows are paged by `_id` (keyset pagination).
    fn page_sql(&self, window: DateWindow, after_id: i64) -> String {
        format!(
            "SELECT _id, station_id, variable_id, reftime, value, v_flag \
             FROM \"{resource}\" \
             WHERE substr(reftime,7,4)||substr(reftime,1,2)||substr(reftime,4,2) \
             BETWEEN '{from}' AND '{to}' AND _id > {after_id} \
             ORDER BY _id LIMIT {limit}",
            resource = self.config.measurements_resource_id,
            from = window.from().format("%Y%m%d"),
            to = window.to().format("%Y%m%d"),
            limit = self.config.page_size,
        )
    }
}

impl<T: Transport> Source for ArpaeSource<T> {
    type Record = Measurement;

    fn fetch(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError> {
        let units = self.fetch_units()?;
        let url = format!(
            "{}/api/3/action/datastore_search_sql",
            self.config.ckan_base_url.trim_end_matches('/')
        );

        let mut measurements = Vec::new();
        let mut after_id = 0;
        // The server may cap a page below `page_size`, so only an empty page ends the scan.
        loop {
            let sql = self.page_sql(window, after_id);
            let body = self.get(&url, &[("sql", &sql)])?;
            let page = parse_nrt_page(&body, &units, self.offset)?;
            let Some(last_id) = page.last_id else { break };
            debug!(rows = page.measurements.len(), last_id, "fetched page");
            measurements.extend(page.measurements);
            after_id = last_id;
        }

        let unknown = measurements.iter().filter(|m| m.unit.is_none()).count();
        if unknown > 0 {
            warn!(
                rows = unknown,
                "measurements of pollutants missing from the registry"
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
