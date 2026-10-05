//! ARPAE Emilia-Romagna source: near-real-time measurements from the CKAN
//! datastore and the station/pollutant registries published as CSV.
//!
//! Formats are documented in `docs/data-exploration.md`.

mod config;
pub mod parse;
mod source;

pub use config::ArpaeConfig;
pub use source::{ArpaeSource, ArpaeStationTypes, ArpaeStations};
