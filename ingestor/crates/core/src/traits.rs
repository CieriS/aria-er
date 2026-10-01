use chrono::NaiveDate;

use crate::{DateWindow, Measurement, SinkError, SourceError, StationSensor};

/// An upstream provider of measurements and of the station registry.
pub trait Source {
    /// All measurements whose source-local reference day falls in `window`.
    fn fetch_measurements(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError>;

    /// Current snapshot of the station registry.
    fn fetch_stations(&self) -> Result<Vec<StationSensor>, SourceError>;
}

/// Outcome of an upsert into the raw layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteReport {
    pub partitions_written: usize,
    /// Rows stored in the touched partitions after the upsert.
    pub rows_stored: usize,
    pub rows_inserted: usize,
    pub rows_updated: usize,
}

/// A destination in the raw layer. Writes must be idempotent.
pub trait Sink {
    /// Upserts by natural key: an incoming row replaces a stored row with the same key.
    fn write_measurements(&self, measurements: &[Measurement]) -> Result<WriteReport, SinkError>;

    /// Stores a registry snapshot for `extracted_on`, replacing a previous one of the same day.
    fn write_stations(
        &self,
        extracted_on: NaiveDate,
        sensors: &[StationSensor],
    ) -> Result<usize, SinkError>;
}
