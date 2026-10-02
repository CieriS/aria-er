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

🚧 Work in progress — phases 0 (data exploration), 1 (Rust ingestor) and 2 (dbt models on
DuckDB) are complete.
See the roadmap below.

## How to run

Requirements: a stable Rust toolchain (edition 2021), [uv](https://docs.astral.sh/uv/) and `make`.

```bash
make ingest                                   # last 30 days (reprocessing window)
make ingest FROM=2026-08-01 TO=2026-08-31     # explicit window, inclusive
make transform                                # dbt build (seeds, snapshot, models, tests) + source freshness
make test                                     # Rust tests (no network) + dbt build with its data tests
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

## Transformations (dbt on DuckDB)

`make transform` builds the warehouse in `warehouse/aria_er.duckdb` from the Parquet files
in `raw/` and the archive samples in `data/samples/`. It needs `make ingest` to have run at
least once.

| Layer | Model | Content |
|---|---|---|
| staging | `stg_arpae__measurements` | Near-real-time rows: typed, UTC timestamp, value in µg/m³ |
| staging | `stg_arpae__measurements_historical` | Validated archive CSVs aligned to the same key |
| staging | `stg_arpae__stations` | Latest extraction of the station registry |
| snapshot | `snap_arpae__stations` | Type 2 history of the registry |
| intermediate | `int_measurements_deduplicated` | Incremental fact, one row per natural key, with quality flags |
| intermediate | `int_measurements_daily` | Daily mean, maximum and coverage |
| intermediate | `int_o3_8h_rolling` | Rolling 8-hour ozone mean |
| intermediate | `int_stations_current` | Current attributes of each station |
| mart | `mart_exceedances_yearly` | Exceedances of each legal limit per station and year |
| mart | `mart_data_completeness` | Coverage per station, pollutant and day, including empty days |

Seeds: `air_quality_limits` (legal thresholds) and `arpae_pollutants` (averaging period and
plausibility bound per pollutant).

Rules applied:

- **Legal day.** Days and years are counted in ARPAE local standard time (UTC+1 all year).
  An hourly value stamped at the end of its hour belongs to the day the hour starts in.
- **Minimum coverage 75%.** A daily mean needs 18 of 24 hours, an 8-hour mean 6 of 8 hours,
  a daily 8-hour maximum 18 of 24 windows. Below that the period is not counted.
- **Exceedance** means strictly greater than the limit.
- **Flag, don't drop.** Negative, zero and implausible values stay in the fact table with
  `is_negative`, `is_zero`, `is_implausible`; only `is_valid` rows enter the aggregates.
  Two tests with `warn` severity surface them without failing the build.
- **Incremental lookback of 30 days**, the same window the ingestor reprocesses.
- **Freshness**: `dbt source freshness` warns when the newest measurement is older than 48 hours.

### Check against the ARPAE annual report (Bologna, 2025)

`mart_exceedances_yearly` compared with ARPAE's
[report on the 2025 data of the Bologna network](https://www.arpae.it/it/il-territorio/bologna/report-a-bo/aria/report-annuali-aria-bo):

| Indicator (2025) | Station | aria-er | ARPAE report |
|---|---|---|---|
| PM10 days above 50 µg/m³ | Porta San Felice | 20 | 20 |
| PM10 days above 50 µg/m³ | Giardini Margherita | 10 | 10 |
| PM10 days above 50 µg/m³ | Via Chiarini | 7 | 7 |
| O3 days with 8-hour maximum above 120 µg/m³ | Giardini Margherita | 27 | 27 |
| O3 days with 8-hour maximum above 120 µg/m³ | Via Chiarini | 54 | 54 |
| NO2 hours above 200 µg/m³ | all three | 0 | 0 |

The same data also reproduce figures the marts do not expose yet: NO2 annual means
(31 / 14 / 16 µg/m³), PM10 annual means (24 / 21 / 17 µg/m³) and ozone hours above
180 µg/m³ (2 at Giardini Margherita, 22 at Via Chiarini).

Notes on the comparison:

- PM10 figures are those of the report's ten-year table. Its monthly table for 2025 lists
  7 for Giardini Margherita and 10 for Chiarini, the opposite of its own ten-year table;
  the data agree with the ten-year table.
- The report states that its times are in standard time, which supports the UTC+1 assumption.
- For ozone the law caps the *three-year average* of exceedance days at 25. The mart
  compares each single year with 25, so `is_over_allowed_exceedances` is an approximation
  for ozone until three full years are loaded.
- 2026 rows cover August only (`year_coverage` ≈ 0.08): their counts are partial.

### Legal limits to verify

The thresholds in `transform/seeds/air_quality_limits.csv` are consistent with the
values quoted in the ARPAE report, but have not been checked line by line against the text of
D.Lgs. 155/2010 in force:

| Pollutant | Metric | Limit | Allowed per year |
|---|---|---|---|
| PM10 | daily mean | 50 µg/m³ | 35 days |
| NO2 | hourly mean | 200 µg/m³ | 18 hours |
| O3 | daily maximum of the 8-hour rolling mean | 120 µg/m³ | 25 days, as a 3-year average (target value, not a limit value) |

Annual-mean limits (PM10 and NO2 40 µg/m³, PM2.5 25 µg/m³) are not in the seed because no
mart uses them yet. Directive (EU) 2024/2881 sets stricter values from 2030.

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

## Design decisions (phase 2)

- **Archive read by dbt for now** ([ADR 0003](docs/adr/0003-historical-archive-read-by-dbt.md)).
  Yearly counts need a full year, which only the validated archive has. Until the ingestor
  can backfill it, a staging model reads the sample CSVs directly; the archive wins over
  the near-real-time feed for the same key.
- **One incremental fact, `delete+insert` on the natural key.** Aggregates on top are
  plain tables: at this volume a full rebuild takes under a second, and incremental
  aggregates would have to handle late revisions.
- **No dbt packages.** The only helper needed, a multi-column uniqueness test, is a
  five-line generic test in the project.
- **Limits and pollutant metadata as seeds.** No threshold, pollutant id or averaging
  period is written in a model; the ozone model selects its pollutant from the seed.
- **Completeness against an expected grid.** Missing hours are absent rows, so coverage
  is computed against every day between the first and last observation of a series.

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
- [x] **2. dbt on DuckDB** — staging + `mart_exceedances_yearly` with tests
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
transform/            dbt project (DuckDB): staging, intermediate, marts, seeds, snapshot, tests
scratch/              throwaway exploration scripts
Makefile              ingest / transform / test / lint
```

## Known limitations

- **No historical backfill yet.** Only the near-real-time datastore (last ~7 weeks) is ingested. The
  historical archive is hosted on Google Drive without a stable API (the file list is an HTML page);
  a window older than the datastore retention simply returns no rows.
- **The source can be stale.** On 2026-10-01 the newest measurement in the datastore was dated
  2026-09-17. The ingestor does not alert on freshness yet.
- Rows deleted upstream are not removed from the raw layer (upsert only), and previous values of
  revised rows are not kept.
- **2025 covers three Bologna stations only**, read from the sample CSVs; 2026 covers August only.
  January–July 2026 is missing from every source and shows as empty days in `mart_data_completeness`.
- Archive files older than the 30-day lookback need `dbt build --full-refresh` to be loaded.
- An 8-hour window ending on a missing hour is not produced; days with many gaps may lack a few windows.
- The station registry has no station type (traffic / background) and the snapshot has a single
  extraction so far, so no history yet.
- dbt tests run against local data, not fixtures; `make test` therefore needs `raw/`.
- Single writer: two concurrent runs on the same partition would race.
- The ARPAE API is slow and intermittently returns 502; runs rely on retries with backoff.
- No source covers early 2026 at the moment: it is not yet in the historical archive and is already outside the near-real-time window.
- The meaning of `v_flag` and the timezone of the timestamps are inferred, not documented by ARPAE.

## License and data

Data © ARPAE Emilia-Romagna, published as open data. Check the license on the dataset page before reuse.
