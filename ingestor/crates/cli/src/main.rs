//! `aq-ingest`: ARPAE air quality measurements to partitioned Parquet.

mod config;
mod pipeline;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use aq_core::{DateWindow, Source};
use aq_http::UreqTransport;
use aq_sink_parquet::{
    LocalStorage, MeasurementSink, StationSnapshotSink, StationTypeSnapshotSink, Storage,
    WeatherSink,
};
use aq_source_arpae::{ArpaeArchive, ArpaeSource};
use aq_source_openmeteo::OpenMeteoSource;
use chrono::{Duration, NaiveDate, Utc};
use clap::{Args, Parser, Subcommand};
use tracing::info;

use crate::config::{Config, LogFormat, SinkConfig, StorageKind};

#[derive(Parser)]
#[command(name = "aq-ingest", version, about)]
struct Cli {
    /// Path to the TOML configuration file.
    #[arg(long, env = "AQ_CONFIG", default_value = "config/default.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ingest ARPAE measurements, the station registry and the station types.
    Run(Window),
    /// Load the validated ARPAE archive files from the local archive directory.
    ///
    /// Without bounds, every year found is loaded.
    Archive(Window),
    /// Ingest hourly Open-Meteo weather at the coordinates of the ARPAE stations.
    Weather(Window),
}

/// A window of days, inclusive. Without bounds, the configured reprocessing
/// window ending today.
#[derive(Args)]
struct Window {
    /// First day, YYYY-MM-DD.
    #[arg(long)]
    from: Option<NaiveDate>,
    /// Last day, YYYY-MM-DD. Defaults to today.
    #[arg(long)]
    to: Option<NaiveDate>,
}

fn init_logging(format: LogFormat) {
    let builder = tracing_subscriber::fmt().with_writer(std::io::stderr);
    match format {
        LogFormat::Json => builder.json().init(),
        LogFormat::Text => builder.init(),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    init_logging(config.log.format);

    // "Today" in the ARPAE clock, which is what its days refer to.
    let today =
        (Utc::now() + Duration::hours(i64::from(config.arpae.utc_offset_hours))).date_naive();
    let resolve = |window: Window| {
        pipeline::resolve_window(
            window.from,
            window.to,
            today,
            config.run.reprocess_window_days,
        )
    };

    match cli.command {
        Command::Run(window) => {
            let window = resolve(window)?;
            run_arpae(config, window)
        }
        Command::Archive(window) => run_archive(config, window.from, window.to),
        Command::Weather(window) => {
            let window = resolve(window)?;
            run_weather(config, window)
        }
    }
}

/// The storage the raw layer is written to, as configured.
fn storage(sink: &SinkConfig) -> Result<Arc<dyn Storage>> {
    match sink.storage {
        StorageKind::Local => Ok(Arc::new(LocalStorage)),
        StorageKind::Gcs => gcs_storage(&sink.gcs_bucket),
    }
}

#[cfg(feature = "gcs")]
fn gcs_storage(bucket: &str) -> Result<Arc<dyn Storage>> {
    anyhow::ensure!(!bucket.is_empty(), "sink.gcs_bucket is empty");
    let storage = aq_sink_parquet::GcsStorage::gcs(bucket)
        .with_context(|| format!("configuring Google Cloud Storage bucket {bucket}"))?;
    Ok(Arc::new(storage))
}

#[cfg(not(feature = "gcs"))]
fn gcs_storage(bucket: &str) -> Result<Arc<dyn Storage>> {
    anyhow::bail!(
        "sink.storage = \"gcs\" (bucket {bucket:?}) needs a binary built with `--features gcs`"
    )
}

fn run_arpae(config: Config, window: DateWindow) -> Result<()> {
    let arpae = ArpaeSource::new(
        UreqTransport::new(&config.http),
        config.arpae,
        config.http.clone(),
    )
    .context("configuring ARPAE source")?;

    let storage = storage(&config.sink)?;
    let station_sink = StationSnapshotSink::with_storage(
        Arc::clone(&storage),
        config.sink.stations_dir,
        Utc::now().date_naive(),
    );
    let stations = pipeline::ingest(&arpae.stations(), &station_sink, window, "station registry")?;

    let type_sink = StationTypeSnapshotSink::with_storage(
        Arc::clone(&storage),
        config.sink.station_types_dir,
        Utc::now().date_naive(),
    );
    let station_types =
        pipeline::ingest(&arpae.station_types(), &type_sink, window, "station types")?;

    let measurement_sink = MeasurementSink::with_storage(storage, config.sink.measurements_dir);
    let measurements = pipeline::ingest(&arpae, &measurement_sink, window, "measurements")?;

    info!(
        from = %window.from(),
        to = %window.to(),
        station_rows = stations.fetched,
        station_type_rows = station_types.fetched,
        fetched = measurements.fetched,
        inserted = measurements.report.rows_inserted,
        updated = measurements.report.rows_updated,
        partitions_written = measurements.report.partitions_written,
        rows_stored = measurements.report.rows_stored,
        "ingestion completed"
    );
    Ok(())
}

fn run_weather(config: Config, window: DateWindow) -> Result<()> {
    // Weather is requested where the stations are: the registry gives the coordinates.
    let arpae = ArpaeSource::new(
        UreqTransport::new(&config.http),
        config.arpae,
        config.http.clone(),
    )
    .context("configuring ARPAE source")?;
    let sensors = arpae
        .stations()
        .fetch(window)
        .context("fetching station registry")?;
    let locations = pipeline::weather_locations(&sensors, config.openmeteo.coordinate_decimals);
    let location_count = locations.len();

    let source = OpenMeteoSource::new(
        UreqTransport::new(&config.http),
        config.openmeteo,
        config.http,
        locations,
    )
    .context("configuring Open-Meteo source")?;
    let sink = WeatherSink::with_storage(storage(&config.sink)?, config.sink.weather_dir);
    let weather = pipeline::ingest(&source, &sink, window, "weather")?;

    info!(
        from = %window.from(),
        to = %window.to(),
        locations = location_count,
        fetched = weather.fetched,
        inserted = weather.report.rows_inserted,
        updated = weather.report.rows_updated,
        partitions_written = weather.report.partitions_written,
        rows_stored = weather.report.rows_stored,
        "ingestion completed"
    );
    Ok(())
}

fn run_archive(config: Config, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Result<()> {
    let archive = ArpaeArchive::new(&config.arpae.archive_dir, config.arpae.utc_offset_hours)
        .context("configuring ARPAE archive")?;
    let sink =
        MeasurementSink::with_storage(storage(&config.sink)?, config.sink.archive_measurements_dir);

    // One year at a time keeps memory bounded whatever the size of the archive.
    let (mut fetched, mut inserted, mut updated, mut partitions) = (0, 0, 0, 0);
    let mut years_loaded = 0;
    for year in archive.years().context("listing archive years")? {
        let Some(window) = pipeline::year_window(year, from, to) else {
            continue;
        };
        let loaded = pipeline::ingest(&archive, &sink, window, "archive measurements")?;
        fetched += loaded.fetched;
        inserted += loaded.report.rows_inserted;
        updated += loaded.report.rows_updated;
        partitions += loaded.report.partitions_written;
        years_loaded += 1;
    }

    info!(
        years = years_loaded,
        fetched,
        inserted,
        updated,
        partitions_written = partitions,
        "ingestion completed"
    );
    Ok(())
}
