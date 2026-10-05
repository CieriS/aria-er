use std::sync::Arc;

use aq_core::StationSensor;
use arrow_array::{ArrayRef, Float64Array, StringArray, UInt32Array};
use arrow_schema::{DataType, Field};

use crate::SnapshotRecord;

impl SnapshotRecord for StationSensor {
    const FILE_NAME: &'static str = "stations.parquet";

    type SortKey = (u32, u32);

    fn sort_key(&self) -> (u32, u32) {
        (self.station.id, self.pollutant.id)
    }

    fn fields() -> Vec<Field> {
        vec![
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
        ]
    }

    fn to_columns(rows: &[&Self]) -> Vec<ArrayRef> {
        let text = |get: fn(&StationSensor) -> &str| -> ArrayRef {
            Arc::new(StringArray::from_iter_values(rows.iter().map(|s| get(s))))
        };
        let number = |get: fn(&StationSensor) -> Option<f64>| -> ArrayRef {
            Arc::new(Float64Array::from_iter(rows.iter().map(|s| get(s))))
        };
        vec![
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
        ]
    }
}
