//! Pure parser for the archive API response. No I/O happens here.

use aq_core::{SourceError, WeatherObservation};
use chrono::NaiveDateTime;
use serde::Deserialize;

use crate::Location;

/// Timestamps are requested in GMT and come without offset.
const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M";
const CONTEXT: &str = "Open-Meteo archive response";

/// The API answers with an object for one location and an array for several.
#[derive(Deserialize)]
#[serde(untagged)]
enum Response {
    One(Box<LocationResponse>),
    Many(Vec<LocationResponse>),
}

#[derive(Deserialize)]
struct LocationResponse {
    latitude: f64,
    longitude: f64,
    utc_offset_seconds: i64,
    hourly: Hourly,
}

#[derive(Deserialize)]
struct Hourly {
    time: Vec<String>,
    temperature_2m: Vec<Option<f64>>,
    precipitation: Vec<Option<f64>>,
    wind_speed_10m: Vec<Option<f64>>,
    wind_direction_10m: Vec<Option<f64>>,
    surface_pressure: Vec<Option<f64>>,
}

fn format_error(message: impl Into<String>) -> SourceError {
    SourceError::Format {
        context: CONTEXT.to_owned(),
        message: message.into(),
    }
}

/// Parses a response for `locations`, which must be in request order.
///
/// Hours for which the provider has no value at all are skipped.
pub fn parse_archive(
    json: &str,
    locations: &[Location],
) -> Result<Vec<WeatherObservation>, SourceError> {
    let response: Response = serde_json::from_str(json).map_err(|e| format_error(e.to_string()))?;
    let responses = match response {
        Response::One(one) => vec![*one],
        Response::Many(many) => many,
    };
    if responses.len() != locations.len() {
        return Err(format_error(format!(
            "{} locations requested, {} returned",
            locations.len(),
            responses.len()
        )));
    }

    let mut observations = Vec::new();
    for (location, response) in locations.iter().zip(responses) {
        if response.utc_offset_seconds != 0 {
            return Err(format_error("timestamps are not in UTC"));
        }
        let hourly = response.hourly;
        let hours = hourly.time.len();
        let series = [
            &hourly.temperature_2m,
            &hourly.precipitation,
            &hourly.wind_speed_10m,
            &hourly.wind_direction_10m,
            &hourly.surface_pressure,
        ];
        if series.iter().any(|values| values.len() != hours) {
            return Err(format_error("hourly series have different lengths"));
        }

        for (i, time) in hourly.time.iter().enumerate() {
            let [temperature_c, precipitation_mm, wind_speed_ms, wind_direction_deg, surface_pressure_hpa] =
                series.map(|values| values[i]);
            if series.iter().all(|values| values[i].is_none()) {
                continue;
            }
            let observed_at = NaiveDateTime::parse_from_str(time, TIME_FORMAT)
                .map_err(|_| format_error(format!("time {time:?}")))?
                .and_utc();
            observations.push(WeatherObservation {
                location_id: location.id.clone(),
                latitude: location.latitude,
                longitude: location.longitude,
                grid_latitude: response.latitude,
                grid_longitude: response.longitude,
                observed_at,
                temperature_c,
                precipitation_mm,
                wind_speed_ms,
                wind_direction_deg,
                surface_pressure_hpa,
            });
        }
    }
    Ok(observations)
}
