use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aq_core::{Measurement, MeasurementKey, SinkError, WriteReport};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, TimestampMicrosecondType, UInt32Type};
use arrow_array::{
    Array, ArrayRef, Float64Array, RecordBatch, StringArray, TimestampMicrosecondArray, UInt32Array,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{DateTime, Datelike};
use tracing::{debug, info};

use crate::{parquet_error, read_batches, schema_error, write_atomically, MEASUREMENTS_FILE};

type Partition = (i32, u32);
type Rows = BTreeMap<MeasurementKey, Measurement>;

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("station_id", DataType::UInt32, false),
        Field::new("pollutant_id", DataType::UInt32, false),
        Field::new(
            "measured_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("unit", DataType::Utf8, true),
        Field::new("validation_flag", DataType::Utf8, false),
        Field::new("raw_reftime", DataType::Utf8, false),
    ]))
}

fn partition_path(root: &Path, (year, month): Partition) -> PathBuf {
    root.join(format!("year={year:04}"))
        .join(format!("month={month:02}"))
        .join(MEASUREMENTS_FILE)
}

pub(crate) fn upsert(root: &Path, measurements: &[Measurement]) -> Result<WriteReport, SinkError> {
    let mut incoming: BTreeMap<Partition, Vec<&Measurement>> = BTreeMap::new();
    for measurement in measurements {
        let at = measurement.measured_at;
        incoming
            .entry((at.year(), at.month()))
            .or_default()
            .push(measurement);
    }

    let mut report = WriteReport::default();
    for (partition, new_rows) in incoming {
        let path = partition_path(root, partition);
        let mut rows = if path.exists() {
            read_partition(&path)?
        } else {
            Rows::new()
        };

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

        write_atomically(&path, schema(), &to_batch(&path, &rows)?)?;
        report.partitions_written += 1;
        info!(
            path = %path.display(),
            rows = rows.len(),
            inserted,
            updated,
            "wrote measurements partition"
        );
    }
    Ok(report)
}

/// Rows are emitted in key order, which makes the file content deterministic.
fn to_batch(path: &Path, rows: &Rows) -> Result<RecordBatch, SinkError> {
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            rows.values().map(|m| m.station_id),
        )),
        Arc::new(UInt32Array::from_iter_values(
            rows.values().map(|m| m.pollutant_id),
        )),
        Arc::new(
            TimestampMicrosecondArray::from_iter_values(
                rows.values().map(|m| m.measured_at.timestamp_micros()),
            )
            .with_timezone("UTC"),
        ),
        Arc::new(Float64Array::from_iter_values(
            rows.values().map(|m| m.value),
        )),
        Arc::new(StringArray::from_iter(
            rows.values().map(|m| m.unit.as_deref()),
        )),
        Arc::new(StringArray::from_iter_values(
            rows.values().map(|m| m.validation_flag.as_str()),
        )),
        Arc::new(StringArray::from_iter_values(
            rows.values().map(|m| m.raw_reftime.as_str()),
        )),
    ];
    RecordBatch::try_new(schema(), columns).map_err(|e| parquet_error(path, e))
}

fn read_partition(path: &Path) -> Result<Rows, SinkError> {
    let mut rows = Rows::new();
    for batch in read_batches(path)? {
        let column = |name: &str| {
            batch
                .column_by_name(name)
                .ok_or_else(|| schema_error(path, format!("missing column {name}")))
        };
        let wrong_type = |name: &str| schema_error(path, format!("unexpected type for {name}"));

        let station_id = column("station_id")?
            .as_primitive_opt::<UInt32Type>()
            .ok_or_else(|| wrong_type("station_id"))?;
        let pollutant_id = column("pollutant_id")?
            .as_primitive_opt::<UInt32Type>()
            .ok_or_else(|| wrong_type("pollutant_id"))?;
        let measured_at = column("measured_at")?
            .as_primitive_opt::<TimestampMicrosecondType>()
            .ok_or_else(|| wrong_type("measured_at"))?;
        let value = column("value")?
            .as_primitive_opt::<Float64Type>()
            .ok_or_else(|| wrong_type("value"))?;
        let unit = column("unit")?
            .as_string_opt::<i32>()
            .ok_or_else(|| wrong_type("unit"))?;
        let validation_flag = column("validation_flag")?
            .as_string_opt::<i32>()
            .ok_or_else(|| wrong_type("validation_flag"))?;
        let raw_reftime = column("raw_reftime")?
            .as_string_opt::<i32>()
            .ok_or_else(|| wrong_type("raw_reftime"))?;

        for i in 0..batch.num_rows() {
            let measurement = Measurement {
                station_id: station_id.value(i),
                pollutant_id: pollutant_id.value(i),
                measured_at: DateTime::from_timestamp_micros(measured_at.value(i))
                    .ok_or_else(|| schema_error(path, "measured_at out of range"))?,
                value: value.value(i),
                unit: (!unit.is_null(i)).then(|| unit.value(i).to_owned()),
                validation_flag: validation_flag.value(i).to_owned(),
                raw_reftime: raw_reftime.value(i).to_owned(),
            };
            rows.insert(measurement.key(), measurement);
        }
    }
    Ok(rows)
}
