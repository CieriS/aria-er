use std::path::PathBuf;
use std::sync::Arc;

use aq_core::{Sink, SinkError, StationSensor, WriteReport};
use arrow_array::types::Date32Type;
use arrow_array::{ArrayRef, Date32Array, Float64Array, RecordBatch, StringArray, UInt32Array};
use arrow_schema::{DataType, Field, Schema};
use chrono::NaiveDate;
use tracing::info;

use crate::{parquet_error, write_atomically};

const STATIONS_FILE: &str = "stations.parquet";

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("station_id", DataType::UInt32, false),
        Field::new("station_name", DataType::Utf8, false),
        Field::new("municipality", DataType::Utf8, false),
        Field::new("province", DataType::Utf8, false),
        Field::new("address", DataType::Utf8, false),
        Field::new("altitude_m", DataType::Float64, true),
        Field::new("longitude", DataType::Float64, true),
        Field::new("latitude", DataType::Float64, true),
        Field::new("pollutant_id", DataType::UInt32, false),
        Field::new("pollutant_name", DataType::Utf8, false),
        Field::new("unit", DataType::Utf8, false),
        Field::new("extracted_on", DataType::Date32, false),
    ]))
}

/// Writes the registry snapshot of one extraction date as
/// `extracted_on=YYYY-MM-DD/stations.parquet`, replacing a snapshot of the same day.
pub struct StationSnapshotSink {
    root: PathBuf,
    extracted_on: NaiveDate,
}

impl StationSnapshotSink {
    pub fn new(root: impl Into<PathBuf>, extracted_on: NaiveDate) -> Self {
        Self {
            root: root.into(),
            extracted_on,
        }
    }
}

impl Sink for StationSnapshotSink {
    type Record = StationSensor;

    fn write(&self, sensors: &[StationSensor]) -> Result<WriteReport, SinkError> {
        let extracted_on = self.extracted_on;
        let path = self
            .root
            .join(format!("extracted_on={extracted_on}"))
            .join(STATIONS_FILE);

        let mut rows: Vec<&StationSensor> = sensors.iter().collect();
        rows.sort_by_key(|s| (s.station.id, s.pollutant.id));

        let text = |get: fn(&StationSensor) -> &str| -> ArrayRef {
            Arc::new(StringArray::from_iter_values(rows.iter().map(|s| get(s))))
        };
        let number = |get: fn(&StationSensor) -> Option<f64>| -> ArrayRef {
            Arc::new(Float64Array::from_iter(rows.iter().map(|s| get(s))))
        };
        let extracted = Date32Type::from_naive_date(extracted_on);

        let columns: Vec<ArrayRef> = vec![
            Arc::new(UInt32Array::from_iter_values(
                rows.iter().map(|s| s.station.id),
            )),
            text(|s| &s.station.name),
            text(|s| &s.station.municipality),
            text(|s| &s.station.province),
            text(|s| &s.station.address),
            number(|s| s.station.altitude_m),
            number(|s| s.station.longitude),
            number(|s| s.station.latitude),
            Arc::new(UInt32Array::from_iter_values(
                rows.iter().map(|s| s.pollutant.id),
            )),
            text(|s| &s.pollutant.name),
            text(|s| &s.pollutant.unit),
            Arc::new(Date32Array::from_iter_values(std::iter::repeat_n(
                extracted,
                rows.len(),
            ))),
        ];
        let batch = RecordBatch::try_new(schema(), columns).map_err(|e| parquet_error(&path, e))?;
        write_atomically(&path, schema(), &batch)?;
        info!(path = %path.display(), rows = rows.len(), "wrote station registry snapshot");
        Ok(WriteReport {
            partitions_written: 1,
            rows_stored: rows.len(),
            rows_inserted: rows.len(),
            rows_updated: 0,
        })
    }
}
