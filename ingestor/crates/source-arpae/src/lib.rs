//! ARPAE Emilia-Romagna source: near-real-time measurements from the CKAN
//! datastore and the station/pollutant registries published as CSV.
//!
//! Formats are documented in `docs/data-exploration.md`.

mod config;
pub mod parse;
mod source;
mod transport;

pub use config::{ArpaeConfig, HttpConfig};
pub use source::ArpaeSource;
pub use transport::{Transport, TransportError, UreqTransport};
