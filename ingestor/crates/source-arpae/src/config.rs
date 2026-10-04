use serde::Deserialize;

/// Where the ARPAE data lives and how to interpret it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArpaeConfig {
    /// CKAN portal root, without trailing slash.
    pub ckan_base_url: String,
    /// Datastore resource holding the near-real-time measurements.
    pub measurements_resource_id: String,
    pub stations_csv_url: String,
    pub pollutants_csv_url: String,
    /// Fixed offset of the published timestamps from UTC, in hours.
    pub utc_offset_hours: i32,
    /// Rows requested per datastore query.
    pub page_size: u32,
}
