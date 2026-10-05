//! Parquet sinks for the raw layer.
//!
//! Time series are partitioned by UTC `year=YYYY/month=MM`, one file per
//! partition. An upsert reads the partitions it touches, merges by natural
//! key, and rewrites them sorted, so that the same input always produces the
//! same bytes (see `docs/adr/0001-parquet-upsert-by-partition-rewrite.md`).

mod measurements;
mod partitioned;
mod snapshot;
mod stations;
mod weather;

use std::fs::{self, File};
use std::path::Path;
use std::sync::Arc;

use aq_core::{Measurement, SinkError, StationSensor, WeatherObservation};
use arrow_array::RecordBatch;
use arrow_schema::Schema;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

pub use partitioned::{PartitionedRecord, PartitionedSink};
pub use snapshot::{SnapshotRecord, SnapshotSink};

/// Sink for ARPAE measurements.
pub type MeasurementSink = PartitionedSink<Measurement>;
/// Sink for dated snapshots of the station registry.
pub type StationSnapshotSink = SnapshotSink<StationSensor>;
/// Sink for hourly weather observations.
pub type WeatherSink = PartitionedSink<WeatherObservation>;

fn io_error(path: &Path, source: std::io::Error) -> SinkError {
    SinkError::Io {
        path: path.display().to_string(),
        source,
    }
}

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

/// Reads every record batch of `path`.
fn read_batches(path: &Path) -> Result<Vec<RecordBatch>, SinkError> {
    let file = File::open(path).map_err(|e| io_error(path, e))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .and_then(|builder| builder.build())
        .map_err(|e| parquet_error(path, e))?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| parquet_error(path, e))
}

/// Writes `batch` to `path` atomically: a reader sees the old file or the new one.
fn write_atomically(
    path: &Path,
    schema: Arc<Schema>,
    batch: &RecordBatch,
) -> Result<(), SinkError> {
    let dir = path
        .parent()
        .ok_or_else(|| schema_error(path, "path has no parent directory"))?;
    fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;

    let tmp = path.with_extension("parquet.tmp");
    let file = File::create(&tmp).map_err(|e| io_error(&tmp, e))?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer =
        ArrowWriter::try_new(file, schema, Some(properties)).map_err(|e| parquet_error(&tmp, e))?;
    writer.write(batch).map_err(|e| parquet_error(&tmp, e))?;
    writer.close().map_err(|e| parquet_error(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io_error(path, e))
}
