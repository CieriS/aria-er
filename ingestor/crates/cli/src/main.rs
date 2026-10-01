//! `aq-ingest`: ARPAE air quality measurements to partitioned Parquet.

mod config;
mod pipeline;

use std::path::PathBuf;

use anyhow::{Context, Result};
use aq_sink_parquet::ParquetSink;
use aq_source_arpae::{ArpaeSource, UreqTransport};
use chrono::{Duration, NaiveDate, Utc};
use clap::{Parser, Subcommand};

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
    /// Ingest a window of days (inclusive, source local time).
    ///
    /// Without bounds, reprocesses the configured window ending today.
    Run {
        /// First day, YYYY-MM-DD.
        #[arg(long)]
        from: Option<NaiveDate>,
        /// Last day, YYYY-MM-DD. Defaults to today.
        #[arg(long)]
        to: Option<NaiveDate>,
    },
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

    match cli.command {
        Command::Run { from, to } => {
            // "Today" in the source's own clock, which is what its days refer to.
            let today = (Utc::now() + Duration::hours(i64::from(config.arpae.utc_offset_hours)))
                .date_naive();
            let window =
                pipeline::resolve_window(from, to, today, config.run.reprocess_window_days)?;

            let transport = UreqTransport::new(&config.http);
            let source = ArpaeSource::new(transport, config.arpae, config.http)
                .context("configuring ARPAE source")?;
            let sink = ParquetSink::new(config.sink.measurements_dir, config.sink.stations_dir);
            pipeline::run(&source, &sink, window, Utc::now().date_naive())
        }
    }
}
