use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use aq_http::HttpConfig;
use aq_source_arpae::ArpaeConfig;
use aq_source_openmeteo::OpenMeteoConfig;
use serde::Deserialize;
use toml::Value;

const ENV_PREFIX: &str = "AQ_";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub arpae: ArpaeConfig,
    pub openmeteo: OpenMeteoConfig,
    pub http: HttpConfig,
    pub sink: SinkConfig,
    pub run: RunConfig,
    pub log: LogConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkConfig {
    pub measurements_dir: PathBuf,
    pub archive_measurements_dir: PathBuf,
    pub stations_dir: PathBuf,
    pub station_types_dir: PathBuf,
    pub weather_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub reprocess_window_days: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    pub format: LogFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Text,
    Json,
}

impl Config {
    /// Loads the TOML file, then applies `AQ_<SECTION>__<KEY>` overrides from the environment.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        Self::parse(&text, std::env::vars())
            .with_context(|| format!("loading config file {}", path.display()))
    }

    fn parse(text: &str, env: impl Iterator<Item = (String, String)>) -> Result<Self> {
        let mut root: Value = toml::from_str(text)?;
        for (name, raw) in env {
            let Some((section, key)) = name
                .strip_prefix(ENV_PREFIX)
                .and_then(|rest| rest.split_once("__"))
            else {
                continue;
            };
            let slot = root
                .get_mut(section.to_lowercase())
                .and_then(|s| s.get_mut(key.to_lowercase()));
            let Some(slot) = slot else {
                bail!("environment variable {name} does not match any config key");
            };
            *slot = match slot {
                Value::Integer(_) => Value::Integer(
                    raw.parse()
                        .with_context(|| format!("{name} must be an integer, got {raw:?}"))?,
                ),
                _ => Value::String(raw),
            };
        }
        Ok(root.try_into()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = include_str!("../../../config/default.toml");

    fn env(pairs: &[(&str, &str)]) -> impl Iterator<Item = (String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn shipped_default_config_is_valid() {
        let config = Config::parse(DEFAULT, env(&[])).unwrap();
        assert_eq!(config.run.reprocess_window_days, 30);
        assert_eq!(config.arpae.utc_offset_hours, 1);
        assert_eq!(config.log.format, LogFormat::Text);
        assert_eq!(config.openmeteo.coordinate_decimals, 1);
    }

    #[test]
    fn environment_overrides_file_values() {
        let config = Config::parse(
            DEFAULT,
            env(&[
                ("AQ_RUN__REPROCESS_WINDOW_DAYS", "45"),
                ("AQ_SINK__MEASUREMENTS_DIR", "/tmp/m"),
                ("AQ_LOG__FORMAT", "json"),
                ("HOME", "/root"),
                ("AQ_CONFIG", "ignored: no section separator"),
            ]),
        )
        .unwrap();
        assert_eq!(config.run.reprocess_window_days, 45);
        assert_eq!(config.sink.measurements_dir, PathBuf::from("/tmp/m"));
        assert_eq!(config.log.format, LogFormat::Json);
    }

    #[test]
    fn unknown_or_malformed_overrides_are_rejected() {
        assert!(Config::parse(DEFAULT, env(&[("AQ_RUN__TYPO", "1")])).is_err());
        assert!(Config::parse(DEFAULT, env(&[("AQ_HTTP__MAX_ATTEMPTS", "many")])).is_err());
    }
}
