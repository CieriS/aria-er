use std::path::Path;
use std::sync::Arc;

use aq_core::{Measurement, MeasurementKey, SinkError};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, TimestampMicrosecondType, UInt32Type};
use arrow_array::{
    Array, ArrayRef, Float64Array, RecordBatch, StringArray, TimestampMicrosecondArray, UInt32Array,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{DateTime, Utc};

use crate::{schema_error, PartitionedRecord};

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
        Field::new("validation_flag", DataType::Utf8, true),
        Field::new("raw_reftime", DataType::Utf8, false),
    ]))
}

impl PartitionedRecord for Measurement {
    type Key = MeasurementKey;

    fn key(&self) -> MeasurementKey {
        Measurement::key(self)
    }

    fn timestamp(&self) -> DateTime<Utc> {
        self.measured_at
    }

    fn schema() -> Arc<Schema> {
        schema()
    }

    fn to_columns(rows: &[&Self]) -> Vec<ArrayRef> {
        vec![
            Arc::new(UInt32Array::from_iter_values(
                rows.iter().map(|m| m.station_id),
            )),
            Arc::new(UInt32Array::from_iter_values(
                rows.iter().map(|m| m.pollutant_id),
            )),
            Arc::new(
                TimestampMicrosecondArray::from_iter_values(
                    rows.iter().map(|m| m.measured_at.timestamp_micros()),
                )
                .with_timezone("UTC"),
            ),
            Arc::new(Float64Array::from_iter_values(rows.iter().map(|m| m.value))),
            Arc::new(StringArray::from_iter(
                rows.iter().map(|m| m.unit.as_deref()),
            )),
            Arc::new(StringArray::from_iter(
                rows.iter().map(|m| m.validation_flag.as_deref()),
            )),
            Arc::new(StringArray::from_iter_values(
                rows.iter().map(|m| m.raw_reftime.as_str()),
            )),
        ]
    }

    fn from_batch(batch: &RecordBatch, path: &Path) -> Result<Vec<Self>, SinkError> {
        let mut rows = Vec::with_capacity(batch.num_rows());
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
                validation_flag: (!validation_flag.is_null(i))
                    .then(|| validation_flag.value(i).to_owned()),
                raw_reftime: raw_reftime.value(i).to_owned(),
            };
            rows.push(measurement);
        }
        Ok(rows)
    }
}
