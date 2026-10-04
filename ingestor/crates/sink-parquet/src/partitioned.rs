use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aq_core::{Sink, SinkError, WriteReport};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::Schema;
use chrono::{DateTime, Datelike, Utc};
use tracing::{debug, info};

use crate::{parquet_error, read_batches, write_atomically};

const PARTITION_FILE: &str = "part-0.parquet";

/// A record that can be stored in month partitions and upserted by key.
pub trait PartitionedRecord: Clone + PartialEq {
    type Key: Ord;

    fn key(&self) -> Self::Key;

    /// Timestamp deciding the partition.
    fn timestamp(&self) -> DateTime<Utc>;

    fn schema() -> Arc<Schema>;

    /// Columns in schema order.
    fn to_columns(rows: &[&Self]) -> Vec<ArrayRef>;

    /// Rows of a batch read back from `path` (used in error messages).
    fn from_batch(batch: &RecordBatch, path: &Path) -> Result<Vec<Self>, SinkError>;
}

/// Upserting sink writing `year=YYYY/month=MM/part-0.parquet` under a root directory.
pub struct PartitionedSink<R> {
    root: PathBuf,
    record: PhantomData<R>,
}

impl<R> PartitionedSink<R> {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            record: PhantomData,
        }
    }

    fn partition_path(&self, (year, month): (i32, u32)) -> PathBuf {
        self.root
            .join(format!("year={year:04}"))
            .join(format!("month={month:02}"))
            .join(PARTITION_FILE)
    }
}

impl<R: PartitionedRecord> Sink for PartitionedSink<R> {
    type Record = R;

    /// Upserts by natural key: an incoming row replaces a stored row with the same key.
    fn write(&self, records: &[R]) -> Result<WriteReport, SinkError> {
        let mut incoming: BTreeMap<(i32, u32), Vec<&R>> = BTreeMap::new();
        for record in records {
            let at = record.timestamp();
            incoming
                .entry((at.year(), at.month()))
                .or_default()
                .push(record);
        }

        let mut report = WriteReport::default();
        for (partition, new_rows) in incoming {
            let path = self.partition_path(partition);
            let mut rows: BTreeMap<R::Key, R> = BTreeMap::new();
            if path.exists() {
                for batch in read_batches(&path)? {
                    for row in R::from_batch(&batch, &path)? {
                        rows.insert(row.key(), row);
                    }
                }
            }

            let (mut inserted, mut updated) = (0, 0);
            for row in new_rows {
                match rows.insert(row.key(), row.clone()) {
                    None => inserted += 1,
                    Some(previous) if previous != *row => updated += 1,
                    Some(_) => {}
                }
            }

            report.rows_stored += rows.len();
            report.rows_inserted += inserted;
            report.rows_updated += updated;
            if inserted == 0 && updated == 0 {
                debug!(path = %path.display(), "partition unchanged, not rewritten");
                continue;
            }

            // Rows are emitted in key order, which makes the file content deterministic.
            let ordered: Vec<&R> = rows.values().collect();
            let batch = RecordBatch::try_new(R::schema(), R::to_columns(&ordered))
                .map_err(|e| parquet_error(&path, e))?;
            write_atomically(&path, R::schema(), &batch)?;
            report.partitions_written += 1;
            info!(
                path = %path.display(),
                rows = rows.len(),
                inserted,
                updated,
                "wrote partition"
            );
        }
        Ok(report)
    }
}
