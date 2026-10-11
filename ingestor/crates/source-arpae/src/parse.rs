//! Pure parsers for the ARPAE formats. No I/O happens here.

use std::collections::{BTreeSet, HashMap, HashSet};

use aq_core::{Measurement, Pollutant, SourceError, Station, StationSensor, StationType};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;

fn format_error(context: &str, message: impl Into<String>) -> SourceError {
    SourceError::Format {
        context: context.to_owned(),
        message: message.into(),
    }
}

/// How the measurement archive maps onto the identifiers of the raw layer.
///
/// The archive has its own station and parameter ids, published as text.
#[derive(Debug, Clone, Default)]
pub struct MeasurementMapping {
    /// Archive station id to canonical station code.
    pub station_ids: HashMap<String, u32>,
    /// Archive parameter id to pollutant id of the parameter registry.
    pub pollutant_ids: HashMap<String, u32>,
    /// Registry ids of the pollutants published as daily means.
    pub daily_pollutant_ids: HashSet<u32>,
}

#[derive(Deserialize)]
struct PageMeta {
    total: usize,
}

#[derive(Deserialize)]
struct MeasurementResponse {
    #[serde(rename = "_items")]
    items: Vec<MeasurementRecord>,
    #[serde(rename = "_meta")]
    meta: PageMeta,
}

#[derive(Deserialize)]
struct MeasurementRecord {
    #[serde(rename = "_id")]
    id: String,
    station_id: String,
    parameter_id: String,
    /// RFC 3339 with the offset, e.g. `2026-09-16T00:00:00+01:00`.
    reftime: String,
    value: f64,
    parameter_unit: String,
    /// Validation stages passed so far, e.g. `Non validato`, `Giornaliera + Mensile`.
    validity: String,
}

/// A measurement with the day its averaging period starts in, in the published offset.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementRow {
    pub period_start_day: NaiveDate,
    pub measurement: Measurement,
}

/// One page of the measurement archive.
#[derive(Debug)]
pub struct MeasurementPage {
    pub rows: Vec<MeasurementRow>,
    /// Records in the page, mapped or not.
    pub records: usize,
    /// Records matching the query across all its pages.
    pub total: usize,
    /// Archive station ids missing from the mapping; their records are left out.
    pub unmapped_stations: BTreeSet<String>,
}

/// Parses a page of the measurement archive (`qa_archivio_dati_public`).
///
/// The archive stamps an hourly value at the start of its hour and a daily value at the
/// end of its day. Both are moved to the convention of the raw layer: the end of the
/// hour, the day itself.
pub fn parse_measurement_page(
    json: &str,
    mapping: &MeasurementMapping,
) -> Result<MeasurementPage, SourceError> {
    const CONTEXT: &str = "measurement archive response";
    let response: MeasurementResponse =
        serde_json::from_str(json).map_err(|e| format_error(CONTEXT, e.to_string()))?;

    let records = response.items.len();
    let mut rows = Vec::with_capacity(records);
    let mut unmapped_stations = BTreeSet::new();
    for record in response.items {
        match mapping.station_ids.get(&record.station_id) {
            Some(&station_id) => rows.push(measurement_row(record, station_id, mapping)?),
            None => {
                unmapped_stations.insert(record.station_id);
            }
        }
    }
    Ok(MeasurementPage {
        rows,
        records,
        total: response.meta.total,
        unmapped_stations,
    })
}

fn measurement_row(
    record: MeasurementRecord,
    station_id: u32,
    mapping: &MeasurementMapping,
) -> Result<MeasurementRow, SourceError> {
    let context = format!("measurement archive record {}", record.id);
    let pollutant_id = *mapping
        .pollutant_ids
        .get(&record.parameter_id)
        .ok_or_else(|| format_error(&context, format!("parameter_id {:?}", record.parameter_id)))?;
    let published = DateTime::parse_from_rfc3339(&record.reftime)
        .map_err(|_| format_error(&context, format!("reftime {:?}", record.reftime)))?;
    let (period_start, reference) = if mapping.daily_pollutant_ids.contains(&pollutant_id) {
        let day = published - Duration::days(1);
        (day, day)
    } else {
        (published, published + Duration::hours(1))
    };

    Ok(MeasurementRow {
        period_start_day: period_start.date_naive(),
        measurement: Measurement {
            station_id,
            pollutant_id,
            measured_at: reference.with_timezone(&Utc),
            value: record.value,
            unit: Some(record.parameter_unit),
            validation_flag: Some(record.validity),
            raw_reftime: record.reftime,
        },
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
    meta: PageMeta,
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
            let updated = DateTime::parse_from_rfc2822(&station.updated)
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
