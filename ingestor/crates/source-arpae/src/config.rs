use std::collections::BTreeMap;

use serde::Deserialize;

/// Where the ARPAE data lives and how to interpret it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArpaeConfig {
    /// Measurement archive of the ARPAE REST service.
    pub measurements_url: String,
    pub stations_csv_url: String,
    /// Station registry of the ARPAE REST service, which carries the type of each station.
    pub station_types_url: String,
    /// Directory holding the yearly archive files (`storico_<year>/*.csv[.gz]`).
    pub archive_dir: String,
    /// Fixed offset of the published timestamps from UTC, in hours.
    pub utc_offset_hours: i32,
    /// Records requested per page of the measurement archive.
    pub page_size: u32,
    /// Registry ids of the pollutants published as daily means.
    pub daily_pollutant_ids: Vec<u32>,
    /// Parameter id of the measurement archive to pollutant id of the parameter registry.
    /// Only the parameters listed here are requested.
    pub pollutant_ids: BTreeMap<String, u32>,
    /// Station id of the measurement archive to canonical station code.
    pub station_ids: BTreeMap<String, u32>,
}
