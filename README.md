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

🚧 Work in progress — phases 0 (data exploration) and 1 (Rust ingestor) are complete.
See the roadmap below.

## How to run

Requirements: a stable Rust toolchain (edition 2021) and `make`.

```bash
make ingest                                   # last 30 days (reprocessing window)
make ingest FROM=2026-08-01 TO=2026-08-31     # explicit window, inclusive
make test                                     # unit and integration tests (no network)
make lint                                     # cargo fmt --check + clippy -D warnings
```

`make ingest` runs `aq-ingest run [--from YYYY-MM-DD] [--to YYYY-MM-DD]`. It downloads
the station registry and the measurements of the window, and writes:

```
raw/arpae/measurements/year=YYYY/month=MM/part-0.parquet
raw/arpae/stations/extracted_on=YYYY-MM-DD/stations.parquet
```

A full month for the whole region is ~113k rows and takes about 4 minutes, almost all of
it waiting for the ARPAE API (~40 s per request).

Configuration lives in [ingestor/config/default.toml](ingestor/config/default.toml)
(endpoints, timeouts, retries, output paths, reprocessing window, log format). Any value
can be overridden with an environment variable `AQ_<SECTION>__<KEY>`, for example
`AQ_RUN__REPROCESS_WINDOW_DAYS=45` or `AQ_LOG__FORMAT=json`.

### Raw measurements schema

| Column | Type | Notes |
|---|---|---|
| `station_id` | uint32 | Canonical station code (`7000014`) |
| `pollutant_id` | uint32 | ARPAE parameter id (5 = PM10, 8 = NO2, …) |
| `measured_at` | timestamp (µs, UTC) | Published reference time converted to UTC |
| `value` | float64 | As published, not converted |
| `unit` | string, nullable | Unit of `value` from the pollutant registry |
| `validation_flag` | string | Source flag, verbatim (`M` / `G`) |
| `raw_reftime` | string | Reference time exactly as published |

Natural key: `(station_id, pollutant_id, measured_at)`. Rows are sorted by key.

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

## Design decisions (phase 1)

- **Upsert by rewriting partitions** ([ADR 0001](docs/adr/0001-parquet-upsert-by-partition-rewrite.md)).
  Each run merges the incoming rows into the monthly files it touches, by natural key,
  and rewrites them sorted. Re-running a window yields byte-identical files and never
  duplicates; a revised value replaces the provisional one.
- **Near-real-time datastore as the only source for now**
  ([ADR 0002](docs/adr/0002-near-real-time-datastore-as-phase-1-source.md)). One SQL query
  per window with keyset pagination; timestamps treated as fixed UTC+1 and converted to
  UTC; the original string is kept so the conversion can be redone.
- **Sources and sinks behind traits** (`Source`, `Sink` in `aq-core`). The HTTP layer is
  a further small trait, so retry, pagination and parsing are tested without network.
- **Blocking HTTP (`ureq`) instead of an async stack.** The ingestor makes a handful of
  sequential requests: an async runtime would add dependencies without any benefit.
- **`arrow` + `parquet` crates instead of Polars or a table format.** Only typed columnar
  write/read is needed; Delta/Iceberg would be oversized at this volume.
- **Fail loudly on unexpected formats.** A row with an unparsable date, value or station
  code aborts the run instead of being skipped.

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
- [x] **1. Minimal Rust ingestor** — ARPAE → partitioned Parquet, idempotent, tested
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
data/samples/arpae/   raw samples downloaded in phase 0 (source of the test fixtures)
docs/                 data exploration report, ADRs
ingestor/             Rust workspace
  config/             default configuration
  crates/core/        domain models, Source/Sink traits, errors
  crates/source-arpae/  ARPAE client, parsing, retry
  crates/sink-parquet/  partitioned Parquet writer with upsert
  crates/cli/         aq-ingest binary
scratch/              throwaway exploration scripts
Makefile              ingest / test / lint
```

## Known limitations

- **No historical backfill yet.** Only the near-real-time datastore (last ~7 weeks) is ingested. The
  historical archive is hosted on Google Drive without a stable API (the file list is an HTML page);
  a window older than the datastore retention simply returns no rows.
- **The source can be stale.** On 2026-10-01 the newest measurement in the datastore was dated
  2026-09-17. The ingestor does not alert on freshness yet.
- Rows deleted upstream are not removed from the raw layer (upsert only), and previous values of
  revised rows are not kept.
- Single writer: two concurrent runs on the same partition would race.
- The ARPAE API is slow and intermittently returns 502; runs rely on retries with backoff.
- No source covers early 2026 at the moment: it is not yet in the historical archive and is already outside the near-real-time window.
- The meaning of `v_flag` and the timezone of the timestamps are inferred, not documented by ARPAE.

## License and data

Data © ARPAE Emilia-Romagna, published as open data. Check the license on the dataset page before reuse.
