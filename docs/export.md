# Daily export

The marts, published once a day as JSON on GitHub Pages, for the aria-er case study of the
portfolio ([ADR 0009](adr/0009-daily-json-export-on-github-pages.md)).

## Where and when

- `https://cieris.github.io/aria-er/data/v1/<file>`, served with
  `Access-Control-Allow-Origin: *` and gzip.
- Written by the [Publish data](../.github/workflows/publish-data.yml) workflow at 06:30 UTC
  and on manual trigger: `make ingest`, `make transform` (models and tests), then the export.
  If any step fails, nothing is deployed and the previous export stays online.
- The raw layer is cached between runs, so a run fetches only the default window (30 days)
  and reloads the archive only when its files change.
- **Months the window never reaches are filled from the raw layer itself**
  (`make fill-gaps`, module `aria_er_orchestration.gaps`): from the day after the last day of
  the historical archive to today, every month without values on its first and last day
  (the current month: on its first day) is fetched, oldest first, at most six per run. No date
  is written anywhere: a new year, or a newer archive, moves the start on its own. A first
  run fills the current year over a couple of days; the cache is saved even when a run fails,
  so fetched months are kept.

Locally: `make export` writes the same files to `site/data/v1/` from `warehouse/aria_er.duckdb`.

## Contract (`v1`)

Every file is a JSON array of flat objects, except `meta.json`. Concentrations are in µg/m³,
floats are rounded to three decimals, and a value that is not a number (a correlation over
too few days) is `null`. Dates are ISO `YYYY-MM-DD` days in ARPAE local standard time
(UTC+1 all year); `generated_at` is UTC. Column meanings are those of the marts, described in
[transformations.md](transformations.md) and in the dbt YAML.

| File | Rows | Source | Columns |
|---|---|---|---|
| `meta.json` | object | — | `schema_version`, `generated_at`, `period` (`from`, `to`: first and last day with data, or `null`), `files` (rows per file), `credits` (`source`, `url`, `licence`, `note`) |
| `exceedances.json` | station × pollutant × metric × year | `mart_exceedances_yearly` | `station_id`, `station_name`, `municipality`, `pollutant_code`, `metric`, `year`, `exceedances`, `limit_ugm3`, `max_exceedances_per_year`, `is_over_allowed_exceedances`, `max_value_ugm3`, `valid_periods`, `year_coverage` |
| `pollutant-trend.json` | municipality × pollutant × year | `mart_pollutant_trend` | `municipality`, `pollutant_code`, `year`, `stations`, `annual_mean_ugm3`, `min_station_mean_ugm3`, `max_station_mean_ugm3`, `min_year_coverage`, `is_representative` |
| `traffic-vs-background.json` | municipality × pollutant × month | `mart_traffic_vs_background` | `municipality`, `pollutant_code`, `month` (first day), `days`, `traffic_stations`, `background_stations`, `traffic_mean_ugm3`, `background_mean_ugm3`, `difference_ugm3`, `traffic_to_background_ratio` |
| `weather-correlation.json` | station × season | `mart_weather_correlation` | `station_id`, `station_name`, `municipality`, `season`, `days`, `pm10_mean_ugm3`, `corr_pm10_wind`, `corr_pm10_precipitation`, `windy_days`, `pm10_mean_windy_ugm3`, `pm10_mean_calm_ugm3`, `rainy_days`, `pm10_mean_rainy_ugm3`, `pm10_mean_dry_ugm3` |
| `completeness-monthly.json` | station × pollutant × month | `mart_data_completeness`, aggregated | `station_id`, `station_name`, `municipality`, `pollutant_code`, `month` (first day), `days` (loaded days in the month), `complete_days`, `completeness` (mean daily share of valid values) |

Size on 2026-10-21, ten years of archive plus 2026: 1 MB for the completeness file, 0.2 MB
or less for the others, before gzip. The export stops if a file exceeds 2 MB.

## Changing the contract

Adding a column or a file is compatible and stays in `v1`. Renaming or removing a column,
changing a unit or the grain of a file is a new version: publish `data/v2/` next to `v1`,
move the portfolio, then stop publishing `v1`. The export tests
(`orchestration/tests/test_export.py`) list the columns of each file, so a mart change that
would break a reader fails there first.

## Licence of the data

ARPAE data are under CC BY and Open-Meteo data under CC BY 4.0; `meta.json` carries the
credits that a page showing these numbers has to display.
