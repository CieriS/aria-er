//! Open-Meteo source: hourly historical weather from the archive API
//! (`/v1/archive`), at a set of deduplicated coordinates.

mod location;
pub mod parse;
mod source;

use serde::Deserialize;

pub use location::{dedup_locations, Location};
pub use source::OpenMeteoSource;

/// Where the Open-Meteo archive lives and how locations are requested.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenMeteoConfig {
    /// Archive endpoint, e.g. `https://archive-api.open-meteo.com/v1/archive`.
    pub archive_url: String,
    /// Decimals coordinates are rounded to; points rounding to the same value are one location.
    pub coordinate_decimals: u32,
    /// Locations sent in a single request.
    pub max_locations_per_request: usize,
}
