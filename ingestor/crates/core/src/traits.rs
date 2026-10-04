use crate::{DateWindow, SinkError, SourceError};

/// An upstream provider of one kind of record.
pub trait Source {
    type Record;

    /// All records whose source-local reference day falls in `window`.
    ///
    /// Sources of undated reference data (e.g. a registry) return their
    /// current snapshot and ignore the window.
    fn fetch(&self, window: DateWindow) -> Result<Vec<Self::Record>, SourceError>;
}

/// Outcome of a write into the raw layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteReport {
    pub partitions_written: usize,
    /// Rows stored in the touched files after the write.
    pub rows_stored: usize,
    pub rows_inserted: usize,
    pub rows_updated: usize,
}

/// A destination in the raw layer for one kind of record. Writes must be idempotent.
pub trait Sink {
    type Record;

    fn write(&self, records: &[Self::Record]) -> Result<WriteReport, SinkError>;
}
