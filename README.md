# aria-er

End-to-end data platform for air quality in Emilia-Romagna, Italy, built on
[ARPAE](https://www.arpae.it) open data and enriched with weather data from Open-Meteo.

## Why

ARPAE publishes air quality measurements across dozens of monitoring stations,
but the data is split across yearly files and APIs, and recent measurements are
provisional and revised after validation. aria-er turns this into a clean,
historized and tested dataset that answers questions like:

- How many days did each station exceed the legal PM10 limit this year?
- How much does traffic contribute compared to background pollution in the same area?
- How much of a pollution peak is explained by weather (wind, rain)?
- Is air quality actually improving over the years?

## What it demonstrates

- **Idempotent, incremental ingestion** written in Rust, handling late-arriving
  and revised data through windowed reprocessing and upserts
- **Layered modeling** with dbt (staging → intermediate → marts), including
  SCD Type 2 history for monitoring stations
- **Data quality as code**: uniqueness, completeness and freshness tests
- **Orchestration** with Dagster: schedules, backfills, asset lineage
- **Reproducible infrastructure**: Docker Compose locally, Terraform on GCP
- **CI** on every pull request across Rust, Python and dbt

## Status

🚧 Work in progress — phase 0 (data exploration) is complete. See the roadmap below.

## Architecture (target)

```
ARPAE API/CSV ─┐
               ├─► ingestor (Rust) ─► raw/ (Parquet, partitioned year/month)
Open-Meteo ────┘                          │
                                          ▼
                              DuckDB (local) | BigQuery (cloud)
                                          │
                                   dbt: staging ─► intermediate ─► marts
                                          │
                          Dagster (schedule, backfill, asset lineage)
                                          │
                                 Streamlit dashboard
```

Only the components of the current phase exist in the repository. The rest is the plan.

## Data sources

All sources come from the ARPAE CKAN portal, dataset
[`qualita-dell-aria-rete-di-monitoraggio`](https://dati.arpae.it/dataset/qualita-dell-aria-rete-di-monitoraggio).

| Source | Access | Coverage |
|---|---|---|
| Near-real-time measurements | CKAN datastore, resource `4dc855a1-6298-4b71-a1ae-d80693d43dcb`, via `datastore_search_sql` | Last ~7 weeks, with a provisional/validated flag |
| Historical measurements | Google Drive folder, one CSV per year × station × pollutant | 2010–2025, validated |
| Station registry | Google Sheets CSV export | Current stations and their pollutants |
| Pollutant registry | Google Sheets CSV export | IDs, units, averaging period |

Full details, schema and data quality findings: [docs/data-exploration.md](docs/data-exploration.md).

## Data quality findings and how they are handled

Phase 0 analysed one year (2025) of data for the Bologna stations. Each issue maps to a design decision:

| Issue found | Handling |
|---|---|
| **Missing hours are absent rows**, not nulls (coverage 94–99.5%, gaps up to 4+ days) | Never fill in raw. Build an expected time grid (station × pollutant × hour/day) and compare it with the actual data to compute completeness in `mart_data_completeness`. Aggregates (e.g. daily means) are only valid above a minimum coverage threshold (75% by EU convention). |
| **Provisional vs validated data** (`v_flag` `M`/`G` in the near-real-time feed, meaning undocumented) | Keep the flag in raw. Reprocess a rolling window (default 30 days) on every run and upsert by natural key `(station_id, pollutant, period_start)`, so revised values replace provisional ones. Validated history overrides near-real-time data for the same key. |
| **Inconsistent formats**: station code as `7000014`, `07000014` or `7.000.014`; dates as `DD/MM` vs `MM/DD`; CO in mg/m³ | Parse each source with its explicit format and fail loudly on mismatch. Normalise the station code to a single canonical integer in staging. Convert units to µg/m³ in staging, keeping the original value and unit in raw. |
| **Many exact zeros** (e.g. 242 hourly NO values at one station) | Likely below the detection limit or rounding. Keep the values and add a quality flag rather than dropping them silently, so the analyses can decide how to treat them. |
| **Timezone not declared**: timestamps do not follow daylight saving (24 hours on DST days) | Treated as fixed-offset local standard time (UTC+1) pending confirmation from ARPAE. Converted to UTC in storage. Europe/Rome is used only for presentation. |

## Planned marts

| Mart | Question |
|---|---|
| `mart_exceedances_yearly` | How many times each station exceeded legal limits, per year |
| `mart_pollutant_trend` | Multi-year trend by pollutant and city |
| `mart_traffic_vs_background` | Traffic vs background stations in the same area |
| `mart_weather_correlation` | Wind/rain vs PM10 peaks |
| `mart_data_completeness` | Data coverage per station (quality audit) |

Legal limits (Italian D.Lgs. 155/2010) will live in a dbt seed, never hardcoded.

## Roadmap

- [x] **0. Exploration** — one year of Bologna data, schema and quality issues documented
- [ ] **1. Minimal Rust ingestor** — ARPAE → partitioned Parquet, idempotent, tested
- [ ] **2. dbt on DuckDB** — staging + `mart_exceedances_yearly` with tests
- [ ] **3. Orchestration** — Dagster daily schedule and backfills
- [ ] **4. Second source** — Open-Meteo + `mart_weather_correlation`
- [ ] **5. Dashboard** — Streamlit
- [ ] **6. Full CI** + Docker Compose
- [ ] **7. Cloud** — GCS + BigQuery via Terraform, same dbt models with a different target
- [ ] **8. (Optional) Streaming** — only if justified in an ADR

A phase is done when it works end-to-end, has tests, and this README is updated.

## Repository layout (current)

```
data/samples/arpae/   raw samples downloaded in phase 0 (used as test fixtures later)
docs/                 data exploration report, ADRs
scratch/              throwaway exploration scripts
```

## Known limitations

- The historical archive is hosted on Google Drive, without a stable API: the file list is read from an HTML page.
- No source covers early 2026 at the moment: it is not yet in the historical archive and is already outside the near-real-time window.
- The meaning of `v_flag` and the timezone of the timestamps are inferred, not documented by ARPAE.

## License and data

Data © ARPAE Emilia-Romagna, published as open data. Check the license on the dataset page before reuse.
