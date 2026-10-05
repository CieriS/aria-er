//! `aq-ingest`: ARPAE air quality measurements to partitioned Parquet.

mod config;
mod pipeline;

use std::path::PathBuf;

use anyhow::{Context, Result};
use aq_core::{DateWindow, Source};
use aq_http::UreqTransport;
use aq_sink_parquet::{MeasurementSink, StationSnapshotSink, StationTypeSnapshotSink, WeatherSink};
use aq_source_arpae::ArpaeSource;
use aq_source_openmeteo::OpenMeteoSource;
use chrono::{Duration, NaiveDate, Utc};
use clap::{Args, Parser, Subcommand};
use tracing::info;

use crate::config::{Config, LogFormat};

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
        Command::Weather(window) => {
            let window = resolve(window)?;
            run_weather(config, window)
        }
    }
}

fn run_arpae(config: Config, window: DateWindow) -> Result<()> {
    let arpae = ArpaeSource::new(
        UreqTransport::new(&config.http),
        config.arpae,
        config.http.clone(),
    )
    .context("configuring ARPAE source")?;

    let station_sink = StationSnapshotSink::new(config.sink.stations_dir, Utc::now().date_naive());
    let stations = pipeline::ingest(&arpae.stations(), &station_sink, window, "station registry")?;

    let type_sink =
        StationTypeSnapshotSink::new(config.sink.station_types_dir, Utc::now().date_naive());
    let station_types =
        pipeline::ingest(&arpae.station_types(), &type_sink, window, "station types")?;

    let measurement_sink = MeasurementSink::new(config.sink.measurements_dir);
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
    let sink = WeatherSink::new(config.sink.weather_dir);
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
