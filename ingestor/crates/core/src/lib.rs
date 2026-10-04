//! Domain models and the `Source` / `Sink` contracts shared by every crate.

mod error;
mod model;
mod traits;

pub use error::{SinkError, SourceError};
pub use model::{
    DateWindow, Measurement, MeasurementKey, Pollutant, Station, StationSensor, WeatherKey,
    WeatherObservation,
};
pub use traits::{Sink, Source, WriteReport};
