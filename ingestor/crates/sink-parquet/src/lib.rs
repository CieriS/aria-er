//! Parquet sinks for the raw layer.
//!
//! Time series are partitioned by UTC `year=YYYY/month=MM`, one file per
//! partition. An upsert reads the partitions it touches, merges by natural
//! key, and rewrites them sorted, so that the same input always produces the
//! same bytes (see `docs/adr/0001-parquet-upsert-by-partition-rewrite.md`).

mod measurements;
mod partitioned;
mod snapshot;
mod station_types;
mod stations;
mod storage;
mod weather;

use std::path::Path;
use std::sync::Arc;

use aq_core::{Measurement, SinkError, StationSensor, StationType, WeatherObservation};
use arrow_array::RecordBatch;
use arrow_schema::Schema;
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

pub use partitioned::{PartitionedRecord, PartitionedSink};
pub use snapshot::{SnapshotRecord, SnapshotSink};
#[cfg(feature = "gcs")]
pub use storage::object::{GcsStorage, ObjectStorage};
pub use storage::{LocalStorage, Storage};

/// Sink for ARPAE measurements.
pub type MeasurementSink = PartitionedSink<Measurement>;
/// Sink for dated snapshots of the station registry.
pub type StationSnapshotSink = SnapshotSink<StationSensor>;
/// Sink for dated snapshots of the station types.
pub type StationTypeSnapshotSink = SnapshotSink<StationType>;
/// Sink for hourly weather observations.
pub type WeatherSink = PartitionedSink<WeatherObservation>;

fn parquet_error(path: &Path, error: impl std::fmt::Display) -> SinkError {
    SinkError::Parquet {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

fn schema_error(path: &Path, message: impl Into<String>) -> SinkError {
    SinkError::Schema {
        path: path.display().to_string(),
        message: message.into(),
    }
}

/// Reads every record batch of the file at `path`; `None` when it does not exist.
fn read_batches(storage: &dyn Storage, path: &Path) -> Result<Option<Vec<RecordBatch>>, SinkError> {
    let Some(data) = storage.read(path)? else {
        return Ok(None);
    };
    let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data))
        .and_then(|builder| builder.build())
        .map_err(|e| parquet_error(path, e))?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
        .map_err(|e| parquet_error(path, e))
}

/// Serialises `batch` as Parquet and stores it at `path`, replacing the file as a whole.
fn write_parquet(
    storage: &dyn Storage,
    path: &Path,
    schema: Arc<Schema>,
    batch: &RecordBatch,
) -> Result<(), SinkError> {
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(Vec::new(), schema, Some(properties))
        .map_err(|e| parquet_error(path, e))?;
    writer.write(batch).map_err(|e| parquet_error(path, e))?;
    let data = writer.into_inner().map_err(|e| parquet_error(path, e))?;
    storage.write(path, data)
}
