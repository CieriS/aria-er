//! The validated ARPAE archive: one CSV per year, station and pollutant, read
//! from a local directory (`storico_<year>/storico_<year>_<station>_<pollutant>.csv[.gz]`).

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use aq_core::{DateWindow, Measurement, Source, SourceError};
use chrono::{Datelike, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use flate2::read::GzDecoder;
use serde::Deserialize;
use tracing::{debug, info};

use crate::parse::parse_station_code;

/// Timestamp format of the archive files (day first, hour only).
const ARCHIVE_TIME_FORMAT: &str = "%d/%m/%Y %H:%M";
const YEAR_DIR_PREFIX: &str = "storico_";

#[derive(Deserialize)]
struct CsvRow {
    #[serde(rename = "COD_STAZ")]
    station: String,
    #[serde(rename = "ID_PARAM")]
    pollutant_id: u32,
    #[serde(rename = "DATA_INIZIO")]
    start: String,
    #[serde(rename = "DATA_FINE")]
    end: String,
    #[serde(rename = "VALORE")]
    value: f64,
    #[serde(rename = "UM")]
    unit: String,
}

/// A parsed archive row with the local day its averaging interval starts on.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveRow {
    pub period_start_day: NaiveDate,
    pub measurement: Measurement,
}

fn format_error(context: &str, message: impl Into<String>) -> SourceError {
    SourceError::Format {
        context: context.to_owned(),
        message: message.into(),
    }
}

fn parse_time(raw: &str, context: &str) -> Result<NaiveDateTime, SourceError> {
    // Files carry the hour only ("01/01/2025 00"): add the minutes to parse it.
    NaiveDateTime::parse_from_str(&format!("{}:00", raw.trim()), ARCHIVE_TIME_FORMAT)
        .map_err(|_| format_error(context, format!("timestamp {raw:?}")))
}

/// Parses one archive file. `context` names the file in error messages.
///
/// The reference time follows the near-real-time feed: the end of the interval
/// for hourly values, the day itself for daily ones.
pub fn parse_archive_csv(
    csv_text: &str,
    offset: FixedOffset,
    context: &str,
) -> Result<Vec<ArchiveRow>, SourceError> {
    let mut reader = csv::Reader::from_reader(csv_text.as_bytes());
    let mut rows = Vec::new();
    for (index, row) in reader.deserialize::<CsvRow>().enumerate() {
        let line = format!("{context} row {}", index + 2);
        let row = row.map_err(|e| format_error(&line, e.to_string()))?;
        let start = parse_time(&row.start, &line)?;
        let end = parse_time(&row.end, &line)?;
        if end <= start {
            return Err(format_error(&line, "interval end is not after its start"));
        }
        let (reference, raw_reftime) = if (end - start).num_hours() >= 24 {
            (start, row.start)
        } else {
            (end, row.end)
        };
        let measured_at = offset
            .from_local_datetime(&reference)
            .single()
            .ok_or_else(|| format_error(&line, format!("timestamp {raw_reftime:?}")))?
            .with_timezone(&Utc);
        rows.push(ArchiveRow {
            period_start_day: start.date(),
            measurement: Measurement {
                station_id: parse_station_code(&row.station)
                    .ok_or_else(|| format_error(&line, format!("COD_STAZ {:?}", row.station)))?,
                pollutant_id: row.pollutant_id,
                measured_at,
                value: row.value,
                unit: Some(row.unit),
                validation_flag: None,
                raw_reftime,
            },
        });
    }
    Ok(rows)
}

/// The archive files found in a local directory.
pub struct ArpaeArchive {
    dir: PathBuf,
    offset: FixedOffset,
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> SourceError {
    SourceError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// Entries of a directory, sorted for a deterministic reading order.
fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
    let mut paths = fs::read_dir(dir)
        .map_err(|e| io_error(dir, e))?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| io_error(dir, e)))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn read_text(path: &Path) -> Result<String, SourceError> {
    let mut text = String::new();
    let file = File::open(path).map_err(|e| io_error(path, e))?;
    let read = if path.extension().is_some_and(|ext| ext == "gz") {
        GzDecoder::new(file).read_to_string(&mut text)
    } else {
        { file }.read_to_string(&mut text)
    };
    read.map_err(|e| io_error(path, e))?;
    Ok(text)
}

fn is_archive_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".csv") || name.ends_with(".csv.gz"))
}

impl ArpaeArchive {
    pub fn new(dir: impl Into<PathBuf>, utc_offset_hours: i32) -> Result<Self, SourceError> {
        let offset = utc_offset_hours
            .checked_mul(3600)
            .and_then(FixedOffset::east_opt)
            .ok_or_else(|| {
                format_error(
                    "ARPAE configuration",
                    format!("utc_offset_hours {utc_offset_hours}"),
                )
            })?;
        Ok(Self {
            dir: dir.into(),
            offset,
        })
    }

    /// Years with a `storico_<year>` directory, in ascending order.
    pub fn years(&self) -> Result<Vec<i32>, SourceError> {
        let mut years: Vec<i32> = sorted_entries(&self.dir)?
            .iter()
            .filter(|path| path.is_dir())
            .filter_map(|path| {
                path.file_name()?
                    .to_str()?
                    .strip_prefix(YEAR_DIR_PREFIX)?
                    .parse()
                    .ok()
            })
            .collect();
        years.sort_unstable();
        Ok(years)
    }
}

impl Source for ArpaeArchive {
    type Record = Measurement;

    /// Measurements whose averaging interval starts on a day of `window`.
    fn fetch(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError> {
        let mut measurements = Vec::new();
        for year in self.years()? {
            if year < window.from().year() || year > window.to().year() {
                continue;
            }
            let year_dir = self.dir.join(format!("{YEAR_DIR_PREFIX}{year}"));
            for path in sorted_entries(&year_dir)? {
                if !is_archive_file(&path) {
                    continue;
                }
                let rows = parse_archive_csv(
                    &read_text(&path)?,
                    self.offset,
                    &path.display().to_string(),
                )?;
                debug!(path = %path.display(), rows = rows.len(), "read archive file");
                measurements.extend(
                    rows.into_iter()
                        .filter(|row| window.contains(row.period_start_day))
                        .map(|row| row.measurement),
                );
            }
        }
        info!(
            from = %window.from(),
            to = %window.to(),
            rows = measurements.len(),
            "read ARPAE archive"
        );
        Ok(measurements)
    }
}
