//! Archive tests on reduced real files taken from `data/samples/arpae/`.

use std::path::PathBuf;

use aq_core::{DateWindow, Source, SourceError};
use aq_source_arpae::{parse_archive_csv, ArpaeArchive};
use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};

const DAILY: &str = include_str!("fixtures/archive/storico_2025/storico_2025_07000014_005.csv");

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/archive")
}

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn window(from: NaiveDate, to: NaiveDate) -> DateWindow {
    DateWindow::new(from, to).unwrap()
}

fn utc_plus_one() -> FixedOffset {
    FixedOffset::east_opt(3600).unwrap()
}

#[test]
fn daily_values_are_stamped_on_their_day_and_carry_the_published_unit() {
    let rows = parse_archive_csv(DAILY, utc_plus_one(), "daily").unwrap();
    assert_eq!(rows.len(), 3);

    let first = &rows[0];
    assert_eq!(first.period_start_day, day(2025, 1, 1));
    let measurement = &first.measurement;
    assert_eq!(
        (measurement.station_id, measurement.pollutant_id),
        (7_000_014, 5)
    );
    // 1 January 00:00 at UTC+1.
    assert_eq!(
        measurement.measured_at,
        Utc.with_ymd_and_hms(2024, 12, 31, 23, 0, 0).unwrap()
    );
    assert_eq!(measurement.value, 32.0);
    assert_eq!(measurement.unit.as_deref(), Some("ug/m3"));
    assert_eq!(measurement.raw_reftime, "01/01/2025 00");
    // The archive is validated and has no flag.
    assert_eq!(measurement.validation_flag, None);
}

#[test]
fn hourly_values_are_stamped_at_the_end_of_the_hour() {
    let csv = "COD_STAZ,ID_PARAM,DATA_INIZIO,DATA_FINE,VALORE,UM\n\
               7000014,8,31/12/2024 23,01/01/2025 00,6,ug/m3\n";
    let rows = parse_archive_csv(csv, utc_plus_one(), "hourly").unwrap();

    assert_eq!(rows[0].period_start_day, day(2024, 12, 31));
    assert_eq!(
        rows[0].measurement.measured_at,
        Utc.with_ymd_and_hms(2024, 12, 31, 23, 0, 0).unwrap()
    );
    assert_eq!(rows[0].measurement.raw_reftime, "01/01/2025 00");
}

#[test]
fn malformed_rows_fail_loudly_naming_the_file_and_row() {
    let header = "COD_STAZ,ID_PARAM,DATA_INIZIO,DATA_FINE,VALORE,UM\n";
    for bad in [
        "7000014,8,2024-12-31 23,01/01/2025 00,6,ug/m3\n",
        "7000014,8,31/12/2024 23,01/01/2025 00,n.d.,ug/m3\n",
        "7000014,8,31/12/2024 23,31/12/2024 23,6,ug/m3\n",
        "7.000,014,8,31/12/2024 23,01/01/2025 00,6,ug/m3\n",
    ] {
        let error =
            parse_archive_csv(&format!("{header}{bad}"), utc_plus_one(), "file.csv").unwrap_err();
        assert!(error.to_string().contains("file.csv row 2"), "{error}");
    }
}

#[test]
fn source_lists_years_and_reads_plain_and_gzipped_files() {
    let archive = ArpaeArchive::new(fixtures(), 1).unwrap();
    assert_eq!(archive.years().unwrap(), [2024, 2025]);

    let all = archive
        .fetch(window(day(2024, 1, 1), day(2025, 12, 31)))
        .unwrap();
    // 25 hourly rows from the gzipped 2024 file, 3 daily rows from the 2025 one.
    assert_eq!(all.len(), 28);
    assert_eq!(all.iter().filter(|m| m.pollutant_id == 8).count(), 25);
}

#[test]
fn source_assigns_the_last_hour_of_the_year_to_the_year_it_starts_in() {
    let archive = ArpaeArchive::new(fixtures(), 1).unwrap();

    let last_day = archive
        .fetch(window(day(2024, 12, 31), day(2024, 12, 31)))
        .unwrap();
    // 24 hours start on 31 December, including the one ending at midnight of 1 January.
    assert_eq!(last_day.len(), 24);
    assert!(last_day.iter().any(|m| m.raw_reftime == "01/01/2025 00"));

    let new_year = archive
        .fetch(window(day(2025, 1, 1), day(2025, 1, 2)))
        .unwrap();
    assert_eq!(new_year.len(), 2);
    assert!(new_year.iter().all(|m| m.pollutant_id == 5));
}

#[test]
fn missing_directory_is_a_readable_error() {
    let archive = ArpaeArchive::new("/nonexistent/archive", 1).unwrap();
    assert!(matches!(archive.years(), Err(SourceError::Io { .. })));
}

#[test]
fn files_that_are_not_archive_csvs_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let year = dir.path().join("storico_2025");
    std::fs::create_dir_all(&year).unwrap();
    std::fs::write(year.join("notes.txt"), "not data").unwrap();
    std::fs::write(year.join("storico_2025_07000014_005.csv"), DAILY).unwrap();
    std::fs::create_dir_all(dir.path().join("other")).unwrap();
    std::fs::write(dir.path().join("anagrafe_stazioni.csv"), "x").unwrap();

    let archive = ArpaeArchive::new(dir.path(), 1).unwrap();
    assert_eq!(archive.years().unwrap(), [2025]);
    assert_eq!(
        archive
            .fetch(window(day(2025, 1, 1), day(2025, 12, 31)))
            .unwrap()
            .len(),
        3
    );
}
