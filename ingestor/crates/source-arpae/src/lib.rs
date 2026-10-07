//! ARPAE Emilia-Romagna source: near-real-time measurements from the CKAN
//! datastore and the station/pollutant registries published as CSV.
//!
//! Formats are documented in `docs/data-exploration.md`.

mod archive;
mod config;
pub mod parse;
mod source;

pub use archive::{parse_archive_csv, ArchiveRow, ArpaeArchive};
pub use config::ArpaeConfig;
pub use source::{ArpaeSource, ArpaeStationTypes, ArpaeStations};
