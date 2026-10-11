//! ARPAE Emilia-Romagna source: measurements from the archive of the ARPAE REST
//! service, the station registries and the yearly files of validated history.
//!
//! Formats are documented in `docs/data.md` and `docs/data-exploration.md`.

mod archive;
mod config;
pub mod parse;
mod source;

pub use archive::{parse_archive_csv, ArchiveRow, ArpaeArchive};
pub use config::ArpaeConfig;
pub use source::{ArpaeSource, ArpaeStationTypes, ArpaeStations};
