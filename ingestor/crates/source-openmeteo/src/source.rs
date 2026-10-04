use aq_core::{DateWindow, Source, SourceError, WeatherObservation};
use aq_http::{get_with_retry, HttpConfig, Transport};
use tracing::{debug, info};

use crate::parse::parse_archive;
use crate::{Location, OpenMeteoConfig};

/// Variables requested, matching the fields of [`WeatherObservation`].
const HOURLY_VARIABLES: &str =
    "temperature_2m,precipitation,wind_speed_10m,wind_direction_10m,surface_pressure";

/// Hourly historical weather at a fixed set of locations.
pub struct OpenMeteoSource<T> {
    transport: T,
    config: OpenMeteoConfig,
    http: HttpConfig,
    locations: Vec<Location>,
}

impl<T: Transport> OpenMeteoSource<T> {
    pub fn new(
        transport: T,
        config: OpenMeteoConfig,
        http: HttpConfig,
        locations: Vec<Location>,
    ) -> Result<Self, SourceError> {
        if config.max_locations_per_request == 0 {
            return Err(SourceError::Format {
                context: "Open-Meteo configuration".to_owned(),
                message: "max_locations_per_request must be positive".to_owned(),
            });
        }
        Ok(Self {
            transport,
            config,
            http,
            locations,
        })
    }

    /// The underlying transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }
}

fn joined(values: impl Iterator<Item = f64>) -> String {
    values.map(|v| v.to_string()).collect::<Vec<_>>().join(",")
}

impl<T: Transport> Source for OpenMeteoSource<T> {
    type Record = WeatherObservation;

    /// Days are UTC days here: the archive is queried in GMT.
    fn fetch(&self, window: DateWindow) -> Result<Vec<WeatherObservation>, SourceError> {
        let (from, to) = (window.from().to_string(), window.to().to_string());
        let mut observations = Vec::new();
        for chunk in self.locations.chunks(self.config.max_locations_per_request) {
            let latitudes = joined(chunk.iter().map(|l| l.latitude));
            let longitudes = joined(chunk.iter().map(|l| l.longitude));
            let query = [
                ("latitude", latitudes.as_str()),
                ("longitude", longitudes.as_str()),
                ("start_date", from.as_str()),
                ("end_date", to.as_str()),
                ("hourly", HOURLY_VARIABLES),
                ("wind_speed_unit", "ms"),
                ("timezone", "GMT"),
            ];
            let body = get_with_retry(
                &self.transport,
                &self.config.archive_url,
                &query,
                &self.http,
            )?;
            let parsed = parse_archive(&body, chunk)?;
            debug!(
                locations = chunk.len(),
                rows = parsed.len(),
                "fetched chunk"
            );
            observations.extend(parsed);
        }
        info!(
            from = %window.from(),
            to = %window.to(),
            locations = self.locations.len(),
            rows = observations.len(),
            "fetched Open-Meteo weather"
        );
        Ok(observations)
    }
}
