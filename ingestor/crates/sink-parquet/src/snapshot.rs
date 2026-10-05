use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

use aq_core::{Sink, SinkError, WriteReport};
use arrow_array::types::Date32Type;
use arrow_array::{ArrayRef, Date32Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use chrono::NaiveDate;
use tracing::info;

use crate::{parquet_error, write_atomically};

/// Reference data stored as one dated file per extraction.
pub trait SnapshotRecord {
    /// File written inside the `extracted_on=YYYY-MM-DD` directory.
    const FILE_NAME: &'static str;

    type SortKey: Ord;

    /// Rows are written in this order, whatever order they arrive in.
    fn sort_key(&self) -> Self::SortKey;

    /// Columns of the record; the sink appends `extracted_on`.
    fn fields() -> Vec<Field>;

    /// Columns in the order of [`SnapshotRecord::fields`].
    fn to_columns(rows: &[&Self]) -> Vec<ArrayRef>;
}

/// Writes the snapshot of one extraction date as
/// `extracted_on=YYYY-MM-DD/<file>`, replacing a snapshot of the same day.
pub struct SnapshotSink<R> {
    root: PathBuf,
    extracted_on: NaiveDate,
    record: PhantomData<R>,
}

impl<R> SnapshotSink<R> {
    pub fn new(root: impl Into<PathBuf>, extracted_on: NaiveDate) -> Self {
        Self {
            root: root.into(),
            extracted_on,
            record: PhantomData,
        }
    }
}

impl<R: SnapshotRecord> Sink for SnapshotSink<R> {
    type Record = R;

    fn write(&self, records: &[R]) -> Result<WriteReport, SinkError> {
        let extracted_on = self.extracted_on;
        let path = self
            .root
            .join(format!("extracted_on={extracted_on}"))
            .join(R::FILE_NAME);

        let mut rows: Vec<&R> = records.iter().collect();
        rows.sort_by_key(|row| row.sort_key());

        let mut fields = R::fields();
        fields.push(Field::new("extracted_on", DataType::Date32, false));
        let schema = Arc::new(Schema::new(fields));

        let mut columns = R::to_columns(&rows);
        columns.push(Arc::new(Date32Array::from_iter_values(
            std::iter::repeat_n(Date32Type::from_naive_date(extracted_on), rows.len()),
        )));

        let batch = RecordBatch::try_new(Arc::clone(&schema), columns)
            .map_err(|e| parquet_error(&path, e))?;
        write_atomically(&path, schema, &batch)?;
        info!(path = %path.display(), rows = rows.len(), "wrote snapshot");
        Ok(WriteReport {
            partitions_written: 1,
            rows_stored: rows.len(),
            rows_inserted: rows.len(),
            rows_updated: 0,
        })
    }
}
