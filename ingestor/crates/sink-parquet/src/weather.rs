use std::path::Path;
use std::sync::Arc;

use aq_core::{SinkError, WeatherKey, WeatherObservation};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, TimestampMicrosecondType};
use arrow_array::{
    Array, ArrayRef, Float64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{DateTime, Utc};

use crate::{schema_error, PartitionedRecord};

type Getter = fn(&WeatherObservation) -> Option<f64>;

/// Nullable variables, in schema order.
const VARIABLES: [(&str, Getter); 5] = [
    ("temperature_c", |w| w.temperature_c),
    ("precipitation_mm", |w| w.precipitation_mm),
    ("wind_speed_ms", |w| w.wind_speed_ms),
    ("wind_direction_deg", |w| w.wind_direction_deg),
    ("surface_pressure_hpa", |w| w.surface_pressure_hpa),
];

impl PartitionedRecord for WeatherObservation {
    type Key = WeatherKey;

    fn key(&self) -> WeatherKey {
        WeatherObservation::key(self)
    }

    fn timestamp(&self) -> DateTime<Utc> {
        self.observed_at
    }

    fn schema() -> Arc<Schema> {
        let mut fields = vec![
            Field::new("location_id", DataType::Utf8, false),
            Field::new("latitude", DataType::Float64, false),
            Field::new("longitude", DataType::Float64, false),
            Field::new("grid_latitude", DataType::Float64, false),
            Field::new("grid_longitude", DataType::Float64, false),
            Field::new(
                "observed_at",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
        ];
        fields.extend(
            VARIABLES
                .iter()
                .map(|(name, _)| Field::new(*name, DataType::Float64, true)),
        );
        Arc::new(Schema::new(fields))
    }

    fn to_columns(rows: &[&Self]) -> Vec<ArrayRef> {
        let required = |get: fn(&WeatherObservation) -> f64| -> ArrayRef {
            Arc::new(Float64Array::from_iter_values(rows.iter().map(|w| get(w))))
        };
        let mut columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from_iter_values(
                rows.iter().map(|w| w.location_id.as_str()),
            )),
            required(|w| w.latitude),
            required(|w| w.longitude),
            required(|w| w.grid_latitude),
            required(|w| w.grid_longitude),
            Arc::new(
                TimestampMicrosecondArray::from_iter_values(
                    rows.iter().map(|w| w.observed_at.timestamp_micros()),
                )
                .with_timezone("UTC"),
            ),
        ];
        for (_, get) in VARIABLES {
            columns.push(Arc::new(Float64Array::from_iter(
                rows.iter().map(|w| get(w)),
            )));
        }
        columns
    }

    fn from_batch(batch: &RecordBatch, path: &Path) -> Result<Vec<Self>, SinkError> {
        let column = |name: &str| {
            batch
                .column_by_name(name)
                .ok_or_else(|| schema_error(path, format!("missing column {name}")))
        };
        let float = |name: &str| {
            column(name)?
                .as_primitive_opt::<Float64Type>()
                .ok_or_else(|| schema_error(path, format!("unexpected type for {name}")))
        };
        let location_id = column("location_id")?
            .as_string_opt::<i32>()
            .ok_or_else(|| schema_error(path, "unexpected type for location_id"))?;
        let observed_at = column("observed_at")?
            .as_primitive_opt::<TimestampMicrosecondType>()
            .ok_or_else(|| schema_error(path, "unexpected type for observed_at"))?;
        let latitude = float("latitude")?;
        let longitude = float("longitude")?;
        let grid_latitude = float("grid_latitude")?;
        let grid_longitude = float("grid_longitude")?;
        let temperature = float("temperature_c")?;
        let precipitation = float("precipitation_mm")?;
        let wind_speed = float("wind_speed_ms")?;
        let wind_direction = float("wind_direction_deg")?;
        let pressure = float("surface_pressure_hpa")?;
        let optional = |array: &Float64Array, i: usize| (!array.is_null(i)).then(|| array.value(i));

        let mut rows = Vec::with_capacity(batch.num_rows());
        for i in 0..batch.num_rows() {
            rows.push(WeatherObservation {
                location_id: location_id.value(i).to_owned(),
                latitude: latitude.value(i),
                longitude: longitude.value(i),
                grid_latitude: grid_latitude.value(i),
                grid_longitude: grid_longitude.value(i),
                observed_at: DateTime::from_timestamp_micros(observed_at.value(i))
                    .ok_or_else(|| schema_error(path, "observed_at out of range"))?,
                temperature_c: optional(temperature, i),
                precipitation_mm: optional(precipitation, i),
                wind_speed_ms: optional(wind_speed, i),
                wind_direction_deg: optional(wind_direction, i),
                surface_pressure_hpa: optional(pressure, i),
            });
        }
        Ok(rows)
    }
}
