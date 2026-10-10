//! Pure parsers for the ARPAE formats. No I/O happens here.

use std::collections::HashMap;

use aq_core::{Measurement, Pollutant, SourceError, Station, StationSensor, StationType};
use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

/// Timestamp format of the near-real-time datastore (month first).
const NRT_REFTIME_FORMAT: &str = "%m/%d/%Y %H:%M";

fn format_error(context: &str, message: impl Into<String>) -> SourceError {
    SourceError::Format {
        context: context.to_owned(),
        message: message.into(),
    }
}

#[derive(Deserialize)]
struct CkanResponse {
    success: bool,
    result: Option<CkanResult>,
}

#[derive(Deserialize)]
struct CkanResult {
    records: Vec<NrtRecord>,
}

#[derive(Deserialize)]
struct NrtRecord {
    #[serde(rename = "_id")]
    id: i64,
    station_id: String,
    variable_id: String,
    reftime: String,
    value: String,
    v_flag: String,
}

/// One page of near-real-time measurements.
#[derive(Debug)]
pub struct NrtPage {
    pub measurements: Vec<Measurement>,
    /// Highest datastore `_id` in the page, used as the cursor for the next one.
    pub last_id: Option<i64>,
}

/// Parses a `datastore_search_sql` response selecting
/// `_id, station_id, variable_id, reftime, value, v_flag`.
///
/// `units` maps pollutant id to its published unit. `offset` is the fixed UTC
/// offset of `reftime`.
pub fn parse_nrt_page(
    json: &str,
    units: &HashMap<u32, String>,
    offset: FixedOffset,
) -> Result<NrtPage, SourceError> {
    const CONTEXT: &str = "datastore response";
    let response: CkanResponse =
        serde_json::from_str(json).map_err(|e| format_error(CONTEXT, e.to_string()))?;
    let result = match response.result {
        Some(result) if response.success => result,
        _ => return Err(format_error(CONTEXT, "CKAN reported success=false")),
    };

    let last_id = result.records.iter().map(|r| r.id).max();
    let measurements = result
        .records
        .into_iter()
        .map(|record| nrt_measurement(record, units, offset))
        .collect::<Result<_, _>>()?;
    Ok(NrtPage {
        measurements,
        last_id,
    })
}

fn nrt_measurement(
    record: NrtRecord,
    units: &HashMap<u32, String>,
    offset: FixedOffset,
) -> Result<Measurement, SourceError> {
    let context = format!("datastore record _id={}", record.id);
    let pollutant_id: u32 = record
        .variable_id
        .trim()
        .parse()
        .map_err(|_| format_error(&context, format!("variable_id {:?}", record.variable_id)))?;
    let value: f64 = record
        .value
        .trim()
        .parse()
        .map_err(|_| format_error(&context, format!("value {:?}", record.value)))?;
    let local = NaiveDateTime::parse_from_str(&record.reftime, NRT_REFTIME_FORMAT)
        .map_err(|_| format_error(&context, format!("reftime {:?}", record.reftime)))?;
    let measured_at = offset
        .from_local_datetime(&local)
        .single()
        .ok_or_else(|| format_error(&context, format!("reftime {:?}", record.reftime)))?
        .with_timezone(&Utc);

    Ok(Measurement {
        station_id: parse_station_code(&record.station_id)
            .ok_or_else(|| format_error(&context, format!("station_id {:?}", record.station_id)))?,
        pollutant_id,
        measured_at,
        value,
        unit: units.get(&pollutant_id).cloned(),
        validation_flag: Some(record.v_flag),
        raw_reftime: record.reftime,
    })
}

/// Normalises the station code, published as `7000014`, `07000014` or `7.000.014`.
pub fn parse_station_code(raw: &str) -> Option<u32> {
    let digits: String = raw.trim().chars().filter(|c| *c != '.').collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[derive(Deserialize)]
struct PollutantRow {
    #[serde(rename = "IdParametro")]
    id: u32,
    #[serde(rename = "PARAMETRO")]
    name: String,
    #[serde(rename = "UM")]
    unit: String,
}

/// Parses the pollutant registry (`Anagrafe parametri`).
pub fn parse_pollutants_csv(csv_text: &str) -> Result<Vec<Pollutant>, SourceError> {
    let mut reader = csv::Reader::from_reader(csv_text.as_bytes());
    reader
        .deserialize::<PollutantRow>()
        .map(|row| {
            let row = row.map_err(|e| format_error("pollutant registry", e.to_string()))?;
            Ok(Pollutant {
                id: row.id,
                name: row.name,
                unit: row.unit,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct StationRow {
    #[serde(rename = "Stazione")]
    name: String,
    #[serde(rename = "Cod_staz")]
    code: String,
    #[serde(rename = "COMUNE")]
    municipality: String,
    #[serde(rename = "INDIRIZZO")]
    address: String,
    #[serde(rename = "PROVINCIA")]
    province: String,
    #[serde(rename = "Altezza")]
    altitude_m: Option<f64>,
    #[serde(rename = "Id_Param")]
    pollutant_id: u32,
    #[serde(rename = "PARAMETRO")]
    pollutant_name: String,
    #[serde(rename = "UM")]
    unit: String,
    #[serde(rename = "LON_GEO")]
    longitude: Option<f64>,
    #[serde(rename = "LAT_GEO")]
    latitude: Option<f64>,
}

/// Parses the station registry (`Anagrafe stazioni`): one row per station and pollutant.
pub fn parse_stations_csv(csv_text: &str) -> Result<Vec<StationSensor>, SourceError> {
    const CONTEXT: &str = "station registry";
    let mut reader = csv::Reader::from_reader(csv_text.as_bytes());
    reader
        .deserialize::<StationRow>()
        .map(|row| {
            let row = row.map_err(|e| format_error(CONTEXT, e.to_string()))?;
            let id = parse_station_code(&row.code)
                .ok_or_else(|| format_error(CONTEXT, format!("Cod_staz {:?}", row.code)))?;
            Ok(StationSensor {
                station: Station {
                    id,
                    name: row.name,
                    municipality: row.municipality,
                    province: row.province,
                    address: row.address,
                    altitude_m: row.altitude_m,
                    longitude: row.longitude,
                    latitude: row.latitude,
                },
                pollutant: Pollutant {
                    id: row.pollutant_id,
                    name: row.pollutant_name,
                    unit: row.unit,
                },
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct TypeRegistryPage {
    #[serde(rename = "_items")]
    items: Vec<TypeRegistryStation>,
    #[serde(rename = "_meta")]
    meta: TypeRegistryMeta,
}

#[derive(Deserialize)]
struct TypeRegistryMeta {
    total: usize,
}

#[derive(Deserialize)]
struct TypeRegistryStation {
    #[serde(rename = "_id")]
    id: String,
    nome: String,
    sigla_provincia: String,
    /// Exposure: `Traffico`, `Fondo`, `Industriale`, or empty.
    tipo_stazione: String,
    /// Area: `Urbana`, `Suburbana`, `Rurale`, or empty.
    zona: String,
    #[serde(rename = "_updated")]
    updated: String,
}

/// Station types from the ARPAE station registry (`qa_stazioni`).
///
/// The label is `<zona> <tipo_stazione>` (e.g. `Urbana Traffico`), the form the daily
/// bulletin used until 2026-10-05, so snapshots taken from either source read the same.
/// A response that does not hold the whole registry is an error: types of the missing
/// stations would silently disappear.
pub fn parse_station_types(json: &str) -> Result<Vec<StationType>, SourceError> {
    const CONTEXT: &str = "station type registry";
    let page: TypeRegistryPage =
        serde_json::from_str(json).map_err(|e| format_error(CONTEXT, e.to_string()))?;
    if page.items.is_empty() {
        return Err(format_error(CONTEXT, "the registry lists no station"));
    }
    if page.items.len() != page.meta.total {
        return Err(format_error(
            CONTEXT,
            format!(
                "{} stations returned out of {}: the registry no longer fits one page",
                page.items.len(),
                page.meta.total
            ),
        ));
    }

    page.items
        .into_iter()
        .map(|station| {
            let station_id = parse_station_code(&station.id)
                .ok_or_else(|| format_error(CONTEXT, format!("_id {:?}", station.id)))?;
            // `_updated` is an HTTP date; its day identifies the version of the record.
            let updated = chrono::DateTime::parse_from_rfc2822(&station.updated)
                .map_err(|_| format_error(CONTEXT, format!("_updated {:?}", station.updated)))?;
            let label = [station.zona.trim(), station.tipo_stazione.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Ok(StationType {
                station_id,
                station_name: station.nome.trim().to_owned(),
                province: station.sigla_provincia.trim().to_owned(),
                type_label: label,
                bulletin_id: updated.format("%Y%m%d").to_string(),
            })
        })
        .collect()
}
