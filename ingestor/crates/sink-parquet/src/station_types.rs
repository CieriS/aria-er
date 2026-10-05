use std::sync::Arc;

use aq_core::StationType;
use arrow_array::{ArrayRef, StringArray, UInt32Array};
use arrow_schema::{DataType, Field};

use crate::SnapshotRecord;

impl SnapshotRecord for StationType {
    const FILE_NAME: &'static str = "station_types.parquet";

    type SortKey = u32;

    fn sort_key(&self) -> u32 {
        self.station_id
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::new("station_id", DataType::UInt32, false),
            Field::new("station_name", DataType::Utf8, false),
            Field::new("province", DataType::Utf8, false),
            Field::new("type_label", DataType::Utf8, false),
            Field::new("bulletin_id", DataType::Utf8, false),
        ]
    }

    fn to_columns(rows: &[&Self]) -> Vec<ArrayRef> {
        let text = |get: fn(&StationType) -> &str| -> ArrayRef {
            Arc::new(StringArray::from_iter_values(rows.iter().map(|t| get(t))))
        };
        vec![
            Arc::new(UInt32Array::from_iter_values(
                rows.iter().map(|t| t.station_id),
            )),
            text(|t| &t.station_name),
            text(|t| &t.province),
            text(|t| &t.type_label),
            text(|t| &t.bulletin_id),
        ]
    }
}
