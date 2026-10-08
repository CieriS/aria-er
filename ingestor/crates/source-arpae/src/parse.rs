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
struct BulletinPage {
    #[serde(rename = "_items")]
    items: Vec<serde_json::Map<String, serde_json::Value>>,
    #[serde(rename = "_links", default)]
    links: BulletinLinks,
}

#[derive(Deserialize, Default)]
struct BulletinLinks {
    last: Option<BulletinLink>,
}

#[derive(Deserialize)]
struct BulletinLink {
    href: String,
}

#[derive(Deserialize)]
struct BulletinVersion {
    data: Vec<BulletinStation>,
}

#[derive(Deserialize)]
struct BulletinStation {
    prov: String,
    idstazione: String,
    stazione: String,
    tipostazione: String,
}

fn bulletin_page(json: &str) -> Result<BulletinPage, SourceError> {
    serde_json::from_str(json).map_err(|e| format_error("bulletin page", e.to_string()))
}

/// Number of the last page of the bulletin listing, from its `_links.last`.
/// `None` when the listing has a single page.
pub fn parse_bulletin_last_page(json: &str) -> Result<Option<u32>, SourceError> {
    let Some(last) = bulletin_page(json)?.links.last else {
        return Ok(None);
    };
    last.href
        .split(['?', '&'])
        .find_map(|parameter| parameter.strip_prefix("page="))
        .and_then(|page| page.parse().ok())
        .map(Some)
        .ok_or_else(|| format_error("bulletin page", format!("last page link {:?}", last.href)))
}

/// Station types of the most recent bulletin in a page of the listing.
///
/// A bulletin holds, per province, a list of versions: the last one is current.
pub fn parse_station_types(json: &str) -> Result<Vec<StationType>, SourceError> {
    const CONTEXT: &str = "bulletin";
    let page = bulletin_page(json)?;
    let latest = page
        .items
        .iter()
        .filter_map(|item| Some((item.get("_id")?.as_str()?, item)))
        .max_by_key(|(id, _)| *id)
        .ok_or_else(|| format_error(CONTEXT, "no bulletin in the page"))?;
    let (bulletin_id, bulletin) = latest;

    let mut types = Vec::new();
    for (key, value) in bulletin {
        // Provinces are the list-valued fields; the rest is bulletin metadata.
        if key.starts_with('_') || !value.is_array() {
            continue;
        }
        let versions: Vec<BulletinVersion> = serde_json::from_value(value.clone())
            .map_err(|e| format_error(CONTEXT, format!("province {key}: {e}")))?;
        let Some(current) = versions.into_iter().last() else {
            continue;
        };
        for station in current.data {
            let station_id = parse_station_code(&station.idstazione).ok_or_else(|| {
                format_error(CONTEXT, format!("idstazione {:?}", station.idstazione))
            })?;
            types.push(StationType {
                station_id,
                station_name: station.stazione.trim().to_owned(),
                province: station.prov.trim().to_owned(),
                type_label: station.tipostazione.trim().to_owned(),
                bulletin_id: bulletin_id.to_owned(),
            });
        }
    }
    if types.is_empty() {
        return Err(format_error(
            CONTEXT,
            format!("bulletin {bulletin_id} lists no station"),
        ));
    }
    Ok(types)
}
